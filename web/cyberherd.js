(() => {
    "use strict";
    const el = (id) => document.getElementById(id);
    const enabled = window.LightningGoatsSite?.cyberherdEnabled === true;
    const key = (v) => typeof v === "string" && /^[0-9a-f]{64}$/.test(v);
    const integer = (v) => Number.isSafeInteger(v) && v >= 0;
    const decimal = (v) => typeof v === "string" && /^(0|[1-9][0-9]{0,18})$/.test(v) && BigInt(v) <= 9223372036854775807n;
    const fraction = (v) => typeof v === "string" && /^(0|[1-9][0-9]{0,25})(\.[0-9]{1,6})?$/.test(v);
    let offset = 0, revision = null, next = null, busy = false;
    const fallback = "/images/lightninggoatslogo1.png";
    function picture(raw) {
        try {
            const url = new URL(raw);
            if (url.protocol === "https:" && !url.username && !url.password && !url.port
                && /^[a-z0-9.-]+\.[a-z]{2,}$/i.test(url.hostname) && !/\.(local|internal|localhost)$/i.test(url.hostname)) return url.href;
        } catch { /* Use the local logo. */ }
        return fallback;
    }
    async function fetchPage(path) {
        const abort = new AbortController(); const timer = setTimeout(() => abort.abort(), 8000);
        try {
            const response = await fetch(path, { signal:abort.signal, redirect:"error",credentials:"omit",cache:"no-store",headers:{Accept:"application/json"} });
            if (response.status === 409) { offset = 0; revision = null; throw new Error("The herd changed. Refreshing…"); }
            if (!response.ok || !response.headers.get("content-type")?.includes("application/json")) throw new Error("The herd is temporarily unavailable.");
            const reader = response.body.getReader(); let size=0; const chunks=[];
            while (true) { const {done,value}=await reader.read(); if(done) break; size+=value.byteLength; if(size>98304) {await reader.cancel();throw new Error("Herd data exceeds its size limit.");} chunks.push(value); }
            const bytes=new Uint8Array(size); let cursor=0; for(const chunk of chunks) {bytes.set(chunk,cursor);cursor+=chunk.length;}
            return JSON.parse(new TextDecoder("utf-8",{fatal:true}).decode(bytes));
        } finally {clearTimeout(timer);abort.abort();}
    }
    function display(data) {
        const r=data?.roster;
        if(data?.version!==1 || !integer(data.generated_at) || Math.abs(Date.now()/1000-data.generated_at)>30 || !r || !key(r.owner)
            || !integer(r.revision) || !integer(r.total_members) || r.total_members>1000 || !integer(r.capacity) || r.capacity<1 || r.capacity>1000
            || r.total_members>r.capacity || !decimal(r.minimum_sats) || !integer(r.member_percent) || r.member_percent>100
            || !Array.isArray(r.members) || r.members.length!==Math.min(50,Math.max(0,r.total_members-offset)) || !["ready","paused","unknown"].includes(data.availability)
            || !(r.next_offset===null || integer(r.next_offset) && r.next_offset>offset && r.next_offset<=r.total_members)) throw new Error("The herd returned invalid data.");
        const body=document.createDocumentFragment();
        r.members.forEach((m,i) => {
            if(!m || m.rank!==offset+i+1 || !key(m.pubkey) || typeof m.display_name!=="string" || !decimal(m.amount_sats) || !fraction(m.points) || !fraction(m.share_percent)) throw new Error("The herd returned invalid member data.");
            const row=document.createElement("tr");
            const rank=document.createElement("td");rank.textContent=String(m.rank);row.append(rank);
            const profile=document.createElement("td");profile.className="profile";
            const img=document.createElement("img");img.alt="";img.referrerPolicy="no-referrer";img.src=picture(m.picture);img.onerror=()=>{img.onerror=null;img.src=fallback;};
            const name=document.createElement("span");name.textContent=m.display_name || "Anon";name.title=m.pubkey;profile.append(img,name);row.append(profile);
            for(const text of [BigInt(m.amount_sats).toLocaleString(),m.points,`${m.share_percent}%`]) {const cell=document.createElement("td");cell.textContent=text;row.append(cell);}
            body.append(row);
        });
        el("members").replaceChildren(body);el("empty").hidden=r.total_members!==0;
        el("summary").textContent=`${r.total_members} of ${r.capacity} places filled · ${r.member_percent}% member allocation`;
        el("joining").textContent=`Qualifying zaps start at ${BigInt(r.minimum_sats) > 0n ? BigInt(r.minimum_sats).toLocaleString() : "1"} sats on a current herd target.`;
        el("capacity").textContent=r.total_members<r.capacity ? "A first like or repost can join while a place is open." : "The herd is full. Qualifying zaps and eligible reposts can headbutt; likes cannot headbutt.";
        el("targets").replaceChildren();
        if(data.targets!==null && (!Array.isArray(data.targets) || data.targets.length>3)) throw new Error("The herd returned invalid targets.");
        for(const target of data.targets || []) {
            if(!target || !key(target.event_id) || ![1,30311,9041].includes(target.kind)) throw new Error("The herd returned invalid targets.");
            const link=document.createElement("a");link.textContent=target.kind===1?"Today’s herd note":target.kind===30311?"Live stream":"Stream goal";
            let address;
            if(target.kind===30311) {
                const parts=String(target.coordinate).split(":");
                if(parts.shift()!=="30311" || !key(parts[0]) || !parts.slice(1).join(":")) throw new Error("The herd returned an invalid stream.");
                address=window.NostrTools.nip19.naddrEncode({kind:30311,pubkey:parts[0],identifier:parts.slice(1).join(":"),relays:[]});
            } else address=window.NostrTools.nip19.neventEncode({id:target.event_id,kind:target.kind,author:r.owner,relays:[]});
            link.href=`nostr:${address}`;el("targets").append(link);
        }
        el("target-status").textContent=data.targets===null?"Current target discovery is unavailable.":data.targets.length?"":"No current herd note or live stream is available.";
        revision=r.revision;next=r.next_offset;el("previous").disabled=offset===0;el("next").disabled=next===null;
        el("status").textContent=data.availability==="ready"?"":"Membership updates are paused or awaiting feeder status.";
        el("herd").hidden=false;
    }
    async function update(reset=false) {
        if(!enabled || busy) return;busy=true;
        if(reset) {offset=0;revision=null;}
        try {
            const query=new URLSearchParams({offset:String(offset),limit:"50"});if(revision!==null) query.set("revision",String(revision));
            display(await fetchPage(`/api/v1/cyberherd?${query}`));
        } catch(error) {el("status").textContent=error.message || "The herd is temporarily unavailable.";el("herd").hidden=true;}
        finally {busy=false;}
    }
    el("previous").addEventListener("click",()=>{if(busy) return;offset=Math.max(0,offset-50);update();});
    el("next").addEventListener("click",()=>{if(!busy && next!==null) {offset=next;update();}});
    const auth=window.LightningGoatsAuth;
    async function connectIdentity() {
        try {await auth.connect();const me=await auth.request("/api/v1/cyberherd/me");if(me.pubkey!==auth.key() || !decimal(me.member?.amount_sats))throw new Error("Invalid membership response.");
            el("self-status").textContent=me.member.active?`Your CyberHerd score: ${BigInt(me.member.amount_sats).toLocaleString()} sats` : me.member.banned?"Your membership is banned.":"You are not an active member this cycle.";
            el("operator-link").hidden=me.operator!==true;
        } catch(error) {el("self-status").textContent=error.message||"Sign-in unavailable.";el("operator-link").hidden=true;}
    }
    if(auth?.enabled()) {el("connect-nostr").hidden=false;el("connect-nostr").addEventListener("click",connectIdentity);}
    document.addEventListener("nlAuth",event=>{if(event.detail?.type==="logout"){el("self-status").textContent="";el("operator-link").hidden=true;}else if(event.detail?.type==="login"&&auth?.enabled())connectIdentity();});
    if(enabled) {update();setInterval(()=>update(true),10000);} else el("status").textContent="CyberHerd is not available yet.";
})();
