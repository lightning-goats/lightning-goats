"""Single-writer launcher and private Stage 2 quote route."""

import asyncio
import fcntl
import hmac
import os
from pathlib import Path
from typing import Annotated
from uuid import UUID

import uvicorn
from coincurve import PublicKey
from pydantic import BaseModel, ConfigDict, Field
from fastapi import Header, HTTPException

state = Path(os.environ["STATE_DIRECTORY"])
credentials = Path(os.environ["CREDENTIALS_DIRECTORY"])
lock = (state / "owner.lock").open("a")
fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)

os.environ["MINT_MNEMONIC"] = (credentials / "mint-seed").read_text().strip()
project_secret = (credentials / "project-quote-token").read_bytes().strip()
if len(project_secret) < 32 or not project_secret.isascii():
    raise RuntimeError("invalid project quote credential")

# Nutshell treats an unset or zero per-invoice limit as unlimited. Preserve
# narrower positive site limits while enforcing the feeder's public ceiling.
MAX_INVOICE_SAT = 1_000_000
for name in ("MINT_MAX_MINT_BOLT11_SAT", "MINT_MAX_MELT_BOLT11_SAT"):
    configured = int(os.environ.get(name, MAX_INVOICE_SAT))
    if configured < 0:
        raise ValueError(f"negative invoice limit in {name}")
    os.environ[name] = str(min(configured or MAX_INVOICE_SAT, MAX_INVOICE_SAT))

# Nutshell reads settings and constructs its ledger on import.
from cashu.core.models import PostMintQuoteRequest, PostMintQuoteResponse
from cashu.mint.app import app
from cashu.mint.startup import ledger

project_lock = asyncio.Lock()


class ProjectQuote(BaseModel):
    model_config = ConfigDict(extra="forbid")
    request_id: UUID
    amount_sat: Annotated[int, Field(strict=True, gt=0, le=MAX_INVOICE_SAT)]
    description_hash: Annotated[str, Field(pattern=r"^[0-9a-f]{64}$")]
    claim_pubkey: Annotated[str, Field(pattern=r"^(02|03)[0-9a-f]{64}$")]


@app.post("/private/v1/project-quotes", response_model=PostMintQuoteResponse)
async def project_quote(payload: ProjectQuote, authorization: str | None = Header(default=None)):
    if not hmac.compare_digest((authorization or "").encode(), b"Bearer " + project_secret):
        raise HTTPException(status_code=401, detail="project quote credential required")
    try:
        PublicKey(bytes.fromhex(payload.claim_pubkey))
    except ValueError:
        raise HTTPException(status_code=422, detail="invalid claim key") from None
    if not payload.request_id.int:
        raise HTTPException(status_code=422, detail="invalid request identity")
    async with project_lock:
        quote = await ledger.mint_quote(
            PostMintQuoteRequest(amount=payload.amount_sat, unit="sat", pubkey=payload.claim_pubkey),
            project_id=f"goats-{payload.request_id}",
            description_hash=bytes.fromhex(payload.description_hash),
        )
    return PostMintQuoteResponse.from_mint_quote(quote)


if __name__ == "__main__":
    uvicorn.run(app, uds=os.environ["LG_MINT_SOCKET"], proxy_headers=False, access_log=False,
        limit_concurrency=32, timeout_keep_alive=5)
