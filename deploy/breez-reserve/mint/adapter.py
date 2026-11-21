"""Nutshell 0.22.1 sat funding through the local reserve service."""

import hashlib
import os
import uuid
from typing import AsyncGenerator

import bolt11
import httpx

from cashu.core.base import Amount, MeltQuote, Unit
from cashu.core.errors import AmountlessInvoiceNotSupportedError, QuoteExpiredError
from cashu.core.models import PostMeltQuoteRequest
from cashu.lightning.base import (
    InvoiceResponse,
    LightningBackend,
    PaymentQuoteResponse,
    PaymentResponse,
    PaymentResult,
    PaymentStatus,
    PaymentStatusResult,
    StatusResponse,
)


class GoatsReserve(LightningBackend):
    supported_units = {Unit.sat}
    supports_description = True

    def __init__(self, unit: Unit, **kwargs):
        self.assert_unit_supported(unit)
        self.unit = unit
        self.socket = os.environ["LG_RESERVE_FUND_SOCKET"]
        self.expected_wallet = os.environ["LG_RESERVE_WALLET"]
        self.network = os.environ["LG_RESERVE_NETWORK"]
        self.currency = {"mainnet": "bc", "regtest": "bcrt"}[self.network]
        self.client = None

    async def startup(self):
        self.client = httpx.AsyncClient(
            transport=httpx.AsyncHTTPTransport(uds=self.socket, retries=0),
            base_url="http://reserve",
            timeout=8,
        )
        health = await self._call("GET", "/health")
        self._verify_wallet(health)

    async def cleanup(self):
        if self.client is not None:
            await self.client.aclose()
            self.client = None

    async def _call(self, method, path, payload=None):
        try:
            response = await self.client.request(method, path, json=payload)
        except httpx.HTTPError:
            raise RuntimeError("reserve request unavailable") from None
        if response.status_code == 409:
            raise ValueError("reserve operation conflicts with its original intent")
        if response.status_code != 200:
            raise RuntimeError(f"reserve returned HTTP {response.status_code}")
        return response.json()

    def _verify_wallet(self, response):
        if (response["wallet"], response["network"]) != (self.expected_wallet, self.network):
            raise RuntimeError("reserve identity mismatch")

    def _invoice(self, request):
        if request != request.lower() and request != request.upper():
            raise ValueError("mixed-case Lightning invoice")
        invoice = bolt11.decode(request.lower())
        if invoice.currency != self.currency:
            raise ValueError("invoice network differs from the reserve network")
        if not invoice.amount_msat or invoice.amount_msat % 1000:
            raise AmountlessInvoiceNotSupportedError("invoice must encode whole sats")
        return invoice

    def validate_payment_request(self, request: str):
        invoice = self._invoice(request)
        if invoice.has_expired():
            raise QuoteExpiredError("invoice expired")
        return invoice.expiry_time

    async def status(self):
        try:
            response = await self._call("GET", "/v1/funding/balance")
            self._verify_wallet(response)
            return StatusResponse(balance=Amount(Unit.sat, response["balance_sat"]))
        except (ValueError, RuntimeError, KeyError):
            return StatusResponse(balance=Amount(Unit.sat, 0), error_message="reserve balance unavailable")

    async def create_invoice(self, amount, memo=None, description_hash=None, *, quote_id, claim_pubkey=None):
        self.assert_unit_supported(amount.unit)
        if description_hash is None:
            description = {"purpose": "mint_quote", "memo": memo or "Cashu mint quote"}
        else:
            if len(description_hash) != 32 or not claim_pubkey:
                raise ValueError("project quote requires a digest and a claim key")
            description = {"purpose": "project_quote", "description_hash": description_hash.hex(), "claim_pubkey": claim_pubkey}
        operation_id = str(uuid.uuid5(uuid.NAMESPACE_URL, f"lightning-goats/mint/receive/{quote_id}"))
        result = await self._call("POST", "/v1/invoices", {
            "operation_id": operation_id, "amount_sat": amount.amount, "expiry_secs": 900,
            "description": description,
        })
        self._verify_wallet(result)
        invoice = self._invoice(result["invoice"])
        if (result["operation_id"], result["payment_hash"], result["amount_sat"], result["expires_at"]) != (
            operation_id, invoice.payment_hash, amount.amount, invoice.expiry_time
        ) or int(invoice.amount_msat) != amount.amount * 1000:
            raise ValueError("reserve invoice does not match the quote")
        if description_hash is not None and invoice.description_hash != description_hash.hex():
            raise ValueError("signed description hash differs from the requested digest")
        return InvoiceResponse(ok=True, checking_id=invoice.payment_hash, payment_request=result["invoice"], amount=amount)

    async def get_invoice_status(self, checking_id):
        try:
            result = await self._call("GET", f"/v1/invoices/{checking_id}")
            if result["payment_hash"] != checking_id:
                raise ValueError("reserve returned a different payment hash")
            return PaymentStatus(result={"settled": PaymentStatusResult.SETTLED, "pending": PaymentStatusResult.PENDING}[result["state"]])
        except (ValueError, RuntimeError, KeyError):
            return PaymentStatus(result=PaymentStatusResult.ERROR)

    async def get_payment_quote(self, melt_quote: PostMeltQuoteRequest):
        invoice = self._invoice(melt_quote.request)
        result = await self._call("POST", "/v1/funding/fee", {"invoice": melt_quote.request.lower()})
        if (result["payment_hash"], result["amount_sat"]) != (invoice.payment_hash, int(invoice.amount_msat) // 1000):
            raise ValueError("reserve fee estimate does not match the invoice")
        return PaymentQuoteResponse(checking_id=invoice.payment_hash,
            fee=Amount(Unit.sat, result["fee_sat"]), amount=Amount(Unit.sat, result["amount_sat"]))

    def get_payment_checking_id(self, quote: MeltQuote):
        # Keep original identity even after a retry of a definitively failed melt.
        return str(uuid.uuid5(uuid.NAMESPACE_URL, f"lightning-goats/mint/melt/{quote.quote}/{quote.request}"))

    async def pay_invoice(self, quote, fee_limit_msat):
        operation_id = self.get_payment_checking_id(quote)
        try:
            if fee_limit_msat % 1000:
                raise ValueError("fee limit must encode whole sats")
            result = await self._call("POST", "/v1/funding/payments", {
                "operation_id": operation_id, "invoice": quote.request.lower(), "max_fee_sat": fee_limit_msat // 1000,
            })
            status = self._outgoing_status(operation_id, result)
            if status.settled:
                invoice = self._invoice(quote.request)
                if (result["payment_hash"], result["amount_sat"]) != (invoice.payment_hash, quote.amount) or status.fee.amount * 1000 > fee_limit_msat:
                    raise ValueError("reserve payment differs from the melt")
            return PaymentResponse(checking_id=operation_id, result=PaymentResult[status.result.name],
                fee=status.fee, preimage=status.preimage)
        except (ValueError, RuntimeError, KeyError):
            # A returned error or transport interruption cannot release proofs.
            return PaymentResponse(checking_id=operation_id, result=PaymentResult.ERROR)

    async def get_payment_status(self, checking_id):
        try:
            result = await self._call("GET", f"/v1/funding/payments/{checking_id}")
            return self._outgoing_status(checking_id, result)
        except (ValueError, RuntimeError, KeyError):
            return PaymentStatus(result=PaymentStatusResult.ERROR)

    @staticmethod
    def _outgoing_status(operation, result):
        if result["operation_id"] != operation:
            raise ValueError("reserve returned a different outgoing operation")
        state = result["state"]
        if state == "settled":
            preimage = result["preimage"]
            if len(preimage) != 64 or hashlib.sha256(bytes.fromhex(preimage)).hexdigest() != result["payment_hash"]:
                raise ValueError("invalid payment proof")
            return PaymentStatus(result=PaymentStatusResult.SETTLED,
                preimage=preimage, fee=Amount(Unit.sat, result["fee_sat"]))
        return PaymentStatus(result={"pending": PaymentStatusResult.PENDING, "failed": PaymentStatusResult.FAILED}[state])

    async def paid_invoices_stream(self) -> AsyncGenerator[str, None]:
        # Nutshell polls the authoritative quote status because this adapter has no stream.
        raise NotImplementedError
