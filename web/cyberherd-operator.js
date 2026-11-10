(() => {
    "use strict";
    const el=id=>document.getElementById(id), auth=window.LightningGoatsAuth;
    let revision=null, offset=0, next=null, templateOffset=0, templateNext=null, pending=null, busy=false;
    const storage="lg-cyberherd-pending-command-v1";
    const canonical=value=>/^(0|[1-9][0-9]{0,18})$/.test(value)&&BigInt(value)<=9223372036854775807n;
    function pendingUI(){for(const id of ["pending","retry","discard"])el(id).hidden=!pending;}
    function message(text){el("message").textContent=text;}
    function lock(value){busy=value;for(const b of document.querySelectorAll("button"))b.disabled=value;el("previous").disabled=value||offset===0;el("next").disabled=value||next===null;el("next-templates").disabled=value||templateNext===null;}
    function clearPending(){pending=null;sessionStorage.removeItem(storage);pendingUI();}
    async function refresh(reset=true){
        if(reset)offset=0;
        const query=new URLSearchParams({offset:String(offset)});if(!reset&&revision!==null)query.set("revision",String(revision));
        const data=await auth.request(`/api/v1/cyberherd/operator/state?${query}`);
        if(!Number.isSafeInteger(data.revision)||!data.settings||!Array.isArray(data.members)||data.members.length>20||!Array.isArray(data.categories)||data.categories.length>100)throw new Error("Invalid operator state.");
        revision=data.revision;next=data.next_offset;
        el("revision").textContent=`Cycle ${data.cycle} · revision ${revision}`;
        el("minimum").value=data.settings.minimum_sats;el("capacity").value=data.settings.max_members;el("percent").value=data.settings.member_percent;
        el("members").replaceChildren();
        for(const m of data.members){
            if(!/^[0-9a-f]{64}$/.test(m.pubkey)||!canonical(m.amount_sats)||!canonical(m.lifetime_sats))throw new Error("Invalid member state.");
            const row=document.createElement("tr"),profile=document.createElement("td"),choose=document.createElement("button");
            choose.textContent=m.profile?.name||m.pubkey.slice(0,12);choose.title=m.pubkey;choose.addEventListener("click",()=>{el("pubkey").value=m.pubkey;el("name").value=m.profile?.name||"";el("lud16").value=m.profile?.lud16||"";el("picture").value=m.profile?.picture||"";el("amount").value=m.amount_sats;});profile.append(choose);row.append(profile);
            for(const text of [m.banned?"Banned":m.active?"Active":"Inactive",BigInt(m.amount_sats).toLocaleString(),BigInt(m.lifetime_sats).toLocaleString()]){const cell=document.createElement("td");cell.textContent=text;row.append(cell);}el("members").append(row);
        }
        const selected=el("category").value;el("category").replaceChildren();
        for(const c of data.categories){const option=document.createElement("option");option.value=c.category;option.textContent=c.category+(c.enabled?"":" (disabled)");el("category").append(option);}if([...el("category").options].some(o=>o.value===selected))el("category").value=selected;
        el("previous").disabled=offset===0;el("next").disabled=next===null;
    }
    async function attempt(){
        if(!pending||pending.actor!==auth.key())throw new Error("Reconnect the signer that created the pending command.");
        const result=await auth.request("/api/v1/cyberherd/operator/command","POST",pending.body);
        if(result.command_id!==JSON.parse(pending.body).id||!Number.isSafeInteger(result.revision))throw new Error("The command receipt did not match; retained for retry.");
        clearPending();await refresh();message("Change saved.");
    }
    async function submit(action){
        if(pending)throw new Error("Resolve the retained pending command first.");
        if(!Number.isSafeInteger(revision))throw new Error("Refresh operator state first.");
        pending={actor:auth.key(),body:JSON.stringify({id:crypto.randomUUID(),expected_revision:revision,action})};
        sessionStorage.setItem(storage,JSON.stringify(pending));pendingUI();await attempt();
    }
    async function run(fn){if(busy)return;lock(true);try{await fn();}catch(e){message(e.message||"Operation unavailable.");}finally{lock(false);}}
    async function connect(){el("operator").hidden=true;revision=null;await auth.connect();const me=await auth.request("/api/v1/cyberherd/me");if(me.pubkey!==auth.key()||me.operator!==true){auth.forget();throw new Error("This signer is not an authorized operator.");}
        el("identity").textContent=`Operator ${me.pubkey}`;el("operator").hidden=false;el("disconnect").hidden=false;
        try{const saved=JSON.parse(sessionStorage.getItem(storage)||"null");if(saved?.actor===auth.key()&&typeof saved.body==="string"&&saved.body.length<=16384)pending=saved;}catch{/* Ignore unrelated browser storage. */}pendingUI();await refresh();message("Connected. Each request is authorized by your Nostr signer.");}
    el("connect").addEventListener("click",()=>run(connect));
    el("disconnect").addEventListener("click",()=>{auth.forget();el("operator").hidden=true;el("disconnect").hidden=true;message("Disconnected.");});
    document.addEventListener("nlAuth",event=>{if(event.detail?.type==="logout"){auth.forget();el("operator").hidden=true;}else if(event.detail?.type==="login")run(connect);});
    el("refresh").addEventListener("click",()=>run(()=>refresh()));
    el("retry").addEventListener("click",()=>run(attempt));
    el("discard").addEventListener("click",()=>{if(confirm("An unacknowledged command may already have succeeded. Have you checked current state before discarding its retry?")){clearPending();message("Pending retry discarded. Current state is unchanged by discarding.");}});
    el("settings").addEventListener("submit",event=>{event.preventDefault();run(async()=>{const minimum=el("minimum").value;if(!canonical(minimum))throw new Error("Use whole sats.");await submit({type:"settings",minimum_sats:minimum,max_members:Number(el("capacity").value),member_percent:Number(el("percent").value)});});});
    el("action").addEventListener("change",()=>{el("profile-fields").hidden=el("action").value!=="activate";el("amount-field").hidden=el("action").value!=="amount";});
    el("member").addEventListener("submit",event=>{event.preventDefault();run(async()=>{const type=el("action").value,pubkey=el("pubkey").value.trim();if(!/^[0-9a-f]{64}$/.test(pubkey))throw new Error("Use a canonical hex public key.");const action={type,pubkey};if(type==="activate")action.profile={name:el("name").value,picture:el("picture").value.trim()||null,lud16:el("lud16").value.trim()||null};if(type==="amount"){if(!canonical(el("amount").value))throw new Error("Use a whole-sat score total.");action.amount_sats=el("amount").value;}await submit(action);});});
    async function loadTemplates(more=false){if(!more){templateOffset=0;el("variants").replaceChildren();}const query=new URLSearchParams({category:el("category").value,offset:String(templateOffset),revision:String(revision)});const data=await auth.request(`/api/v1/cyberherd/operator/templates?${query}`);if(!Array.isArray(data.variants)||data.variants.length>5)throw new Error("Invalid template page.");el("category-state").textContent=data.enabled?"Category enabled":"Category disabled";templateNext=data.next_offset;
        for(const v of data.variants){const button=document.createElement("button");button.textContent=v.variant+(v.deleted?" (deleted)":"");button.addEventListener("click",()=>{el("variant").value=v.variant;el("template-text").value=v.text||"";});el("variants").append(button);}el("next-templates").disabled=templateNext===null;}
    el("load-templates").addEventListener("click",()=>run(()=>loadTemplates()));
    el("next-templates").addEventListener("click",()=>run(async()=>{if(templateNext!==null){templateOffset=templateNext;await loadTemplates(true);}}));
    el("template-action").addEventListener("change",()=>{el("variant").required=["Set","Delete"].includes(el("template-action").value);});
    el("template").addEventListener("submit",event=>{event.preventDefault();run(()=>{const kind=el("template-action").value,edit={category:el("category").value};if(["Set","Delete"].includes(kind))edit.variant=el("variant").value;if(kind==="Set")edit.text=el("template-text").value;return submit({type:"template",edit:{[kind]:edit}});});});
    el("reset").addEventListener("click",()=>{if(confirm("Start a new membership cycle and clear current scores and social awards?"))run(()=>submit({type:"reset"}));});
    el("previous").addEventListener("click",()=>run(async()=>{offset=Math.max(0,offset-20);await refresh(false);}));
    el("next").addEventListener("click",()=>run(async()=>{if(next!==null){offset=next;await refresh(false);}}));
    if(auth.enabled()){el("connect").hidden=false;message("Connect an authorized Nostr signer to use operator tools.");}
})();
