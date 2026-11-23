"""Prepare and retain one signed public reward from its verified collector disposition."""
import hashlib
import json
from uuid import UUID

from coincurve import PublicKeyXOnly
from operator_delivery import public_reward_tags


def publication(store, request):
    if set(request) not in ({"feed","member","terms_hash"},{"feed","member","terms_hash","event"}):
        raise ValueError("invalid public signing request")
    disposition = f"member-ecash:{UUID(request['feed'])}:{request['member']}"
    with store.write():
        row = store.db.execute("""
            SELECT m.token,m.amount_sat,p.binding FROM member_ecash m JOIN proof_disposition p ON m.feed=p.id
            WHERE m.feed=? AND m.token IS NOT NULL AND p.purpose='cyberherd_payout'
            AND json_extract(p.binding,'$.assignment.terms_hash')=?
            AND json_extract(p.binding,'$.assignment.destination.method')='public_nutzap'
        """,(disposition,request["terms_hash"])).fetchone()
        if row is None:
            raise ValueError("missing public reward")
        token,amount,binding = row
        assignment = json.loads(binding)["assignment"]
        tags = public_reward_tags(token,amount,assignment)
        store.db.execute("INSERT INTO member_publications(feed) VALUES(?) ON CONFLICT(feed) DO NOTHING",(disposition,))
        stamp,saved = store.db.execute("SELECT created_at,signed_event FROM member_publications WHERE feed=?",(disposition,)).fetchone()
        owner = assignment["owner"]
        unsigned = {"kind":9321,"created_at":stamp,"content":"","tags":tags}
        if "event" in request:
            event = request["event"]
            if (set(event) != set(unsigned)|{"pubkey","id","sig"} or event["pubkey"] != owner
                    or any(event[key] != value for key,value in unsigned.items())):
                raise ValueError("changed public reward event")
            raw = json.dumps([0,owner,stamp,9321,tags,""],ensure_ascii=False,separators=(",",":"))
            digest = hashlib.sha256(raw.encode()).digest()
            if event["id"] != digest.hex() or not PublicKeyXOnly(bytes.fromhex(owner)).verify(bytes.fromhex(event["sig"]),digest):
                raise ValueError("invalid public reward signature")
            if saved is None:
                saved = json.dumps(event,separators=(",",":"))
                store.db.execute("UPDATE member_publications SET signed_event=? WHERE feed=?",(saved,disposition))
        return {"owner":owner,"amount_sats":amount,"unsigned":unsigned,"signed":saved is not None}


def serve():
    import os
    import pwd
    import socket
    from claim_store import ClaimStore
    from invoice_server import handle_request
    if int(os.environ.get("LISTEN_PID",0)) != os.getpid() or os.environ.get("LISTEN_FDS") != "1":
        raise RuntimeError("one systemd public reward socket required")
    uid = pwd.getpwnam("cyberherd-signer").pw_uid
    if uid == 0:
        raise ValueError("public reward signer must be non-root")
    store = ClaimStore("/var/lib/lightning-goats-project-collector")
    with socket.socket(fileno=3) as listener:
        while True:
            connection,_ = listener.accept()
            with connection:
                try:
                    handle_request(connection,uid,lambda req: publication(store,req))
                except (TimeoutError, ConnectionError):
                    continue


if __name__ == "__main__":
    serve()
