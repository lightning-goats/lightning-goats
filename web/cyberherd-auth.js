/* Existing NIP-07 / Nostr Connect signer only. Auth events never go to relays. */
(() => {
    "use strict";
    let signer = null, pubkey = "";
    const enabled = () => window.LightningGoatsSite?.cyberherdEnabled === true && window.LightningGoatsSite?.cyberherdAuthEnabled === true;
    function forget() { signer = null; pubkey = ""; }
    async function connect() {
        if (!enabled()) throw new Error("Nostr sign-in is not enabled yet.");
        const extension = window.__lightningGoatsNip07;
        signer = extension?.getPublicKey && extension?.signEvent ? extension : window.nostr;
        if (!signer?.getPublicKey || !signer?.signEvent) {
            document.dispatchEvent(new CustomEvent("nlLaunch", {detail:"login"}));
            throw new Error("Choose your Nostr extension or Nostr Connect signer.");
        }
        try {
            const key = await signer.getPublicKey();
            if (!/^[0-9a-f]{64}$/.test(key)) throw new Error("The signer returned an invalid identity.");
            pubkey = key; return key;
        } catch (error) {forget();throw error;}
    }
    async function request(path, method = "GET", body = null) {
        if (!enabled() || !signer || !pubkey) throw new Error("Connect your Nostr signer first.");
        const url = new URL(path,location.origin);
        if (url.origin !== location.origin || !url.pathname.startsWith("/api/v1/cyberherd/") || !["GET","POST"].includes(method)) throw new Error("Invalid authenticated request.");
        const tags = [["u",url.href],["method",method],["nonce",crypto.randomUUID()]];
        if (method === "POST") {
            if (typeof body !== "string" || new TextEncoder().encode(body).length > 16384) throw new Error("Command exceeds its size limit.");
            const digest = await crypto.subtle.digest("SHA-256",new TextEncoder().encode(body));
            tags.push(["payload",Array.from(new Uint8Array(digest),b=>b.toString(16).padStart(2,"0")).join("")]);
        }
        const unsigned = {kind:27235,created_at:Math.floor(Date.now()/1000),content:"",tags};
        const account = pubkey;
        const expected=JSON.stringify([unsigned.kind,unsigned.created_at,unsigned.content,unsigned.tags]);
        const signed = await signer.signEvent(structuredClone(unsigned));
        if (account !== pubkey || signed.pubkey !== account || JSON.stringify([signed.kind,signed.created_at,signed.content,signed.tags]) !== expected || !window.NostrTools.verifyEvent(signed)) throw new Error("The signer changed the authorized request.");
        const bytes = new TextEncoder().encode(JSON.stringify(signed));
        const authorization = "Nostr " + btoa(Array.from(bytes,b=>String.fromCharCode(b)).join(""));
        const controller = new AbortController(), timer = setTimeout(()=>controller.abort(),8000);
        try {
            const response = await fetch(url.href,{method,body:method==="POST"?body:undefined,headers:{Authorization:authorization,Accept:"application/json",...(method==="POST"?{"Content-Type":"application/json"}:{})},redirect:"error",credentials:"omit",cache:"no-store",signal:controller.signal});
            if (!response.ok) {
                const message = response.status===401?"The request was not authorized. Check the selected signer and your clock.":response.status===409?(method==="GET"?"The herd changed. Refresh and try again.":"The command was held. Refresh state and check revision, capacity, ban and feeder status before retrying."):(method==="GET"?"Your CyberHerd status is temporarily unavailable. Try again shortly.":"The CyberHerd service is unavailable. An unacknowledged command may already have succeeded.");
                throw new Error(message);
            }
            if (!response.headers.get("content-type")?.includes("application/json")) throw new Error("The service returned invalid data.");
            const reader=response.body.getReader(), chunks=[];let size=0;
            while(true) {const {done,value}=await reader.read();if(done)break;size+=value.byteLength;if(size>98304){await reader.cancel();throw new Error("Response exceeds its size limit.");}chunks.push(value);}
            const all=new Uint8Array(size);let offset=0;for(const chunk of chunks){all.set(chunk,offset);offset+=chunk.length;}
            return JSON.parse(new TextDecoder("utf-8",{fatal:true}).decode(all));
        } finally {clearTimeout(timer);controller.abort();}
    }
    document.addEventListener("nlAuth",event=>{if(event.detail?.type==="logout")forget();});
    window.LightningGoatsAuth=Object.freeze({enabled,connect,request,forget,key:()=>pubkey});
})();
