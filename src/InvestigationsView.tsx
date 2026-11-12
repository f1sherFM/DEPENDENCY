import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Investigation, InvestigationMetadata, InvestigationPage, SavedGraphState } from "./types";

export function InvestigationsView({ projectId, currentSnapshotId, revision, busy, onOpen, onEdit, onDeleted }: { projectId: string; currentSnapshotId: string; revision: number; busy: boolean; onOpen: (id: string) => void; onEdit: (id: string) => void; onDeleted: (id: string) => void }) {
  const [page,setPage]=useState<InvestigationPage>({items:[],total:0,nextOffset:null});
  const [error,setError]=useState(""); const [loading,setLoading]=useState(false); const [confirm,setConfirm]=useState<string|null>(null);
  const generation=useRef(0);
  const fetchPage=async(offset=0)=>{
    const token=++generation.current; setLoading(true); setError("");
    try {const next=await invoke<InvestigationPage>("list_investigations",{projectId,offset,limit:20});if(token===generation.current)setPage(old=>({...next,items:offset?[...old.items,...next.items]:next.items}));}
    catch(reason){if(token===generation.current)setError(String(reason));}
    finally{if(token===generation.current)setLoading(false);}
  };
  useEffect(()=>{setPage({items:[],total:0,nextOffset:null});setConfirm(null);void fetchPage();return()=>{generation.current++;};},[projectId,revision]);
  const remove=async(item:InvestigationMetadata)=>{
    setLoading(true);setError("");
    try{await invoke("delete_investigation",{id:item.id,expectedRevision:item.revision});onDeleted(item.id);setConfirm(null);await fetchPage();}
    catch(reason){setError(String(reason));setConfirm(null);setLoading(false);}
  };
  return <section className="panel investigations-view"><div className="investigation-heading"><h2>Сохранённые исследования</h2><button className="text-button" disabled={loading} onClick={()=>void fetchPage()}>Обновить список</button></div><p>Исследование хранит заметку и обычный граф на конкретном снимке. Новый анализ не изменяет сохранённое исследование.</p>
    {error&&<div role="alert" className="error-banner">{error}</div>}
    {!loading&&!page.items.length&&!error&&<p>Исследований пока нет. Настройте граф и нажмите «Сохранить исследование».</p>}
    <div className="investigation-list">{page.items.map(item=><article className="investigation-item" key={item.id}><div><h3>{item.name}</h3><p className="investigation-note">{item.notePreview||"Без заметки"}</p><small>Снимок {item.snapshotId.slice(0,8)} · {item.snapshotId===currentSnapshotId?"открыт сейчас":"другой снимок"} · {new Date(item.updatedAt).toLocaleString("ru-RU")}</small></div><div className="investigation-actions">
      <button disabled={busy||loading||item.stateVersion!==1} onClick={()=>onOpen(item.id)}>Открыть исследование</button><button disabled={busy||loading||item.stateVersion!==1} onClick={()=>onEdit(item.id)}>Изменить заметку</button>
      {confirm===item.id?<><span>Удалить только исследование? Снимок останется.</span><button disabled={busy||loading} onClick={()=>void remove(item)}>Да, удалить</button><button disabled={loading} onClick={()=>setConfirm(null)}>Отмена</button></>:<button disabled={busy||loading||item.stateVersion!==1} onClick={()=>setConfirm(item.id)}>Удалить</button>}
      {item.stateVersion!==1&&<span>Формат исследования недоступен в этой версии приложения.</span>}
    </div></article>)}</div>
    <p>{page.items.length} из {page.total}{loading&&" · Загружаем…"}</p>{page.nextOffset!==null&&<button className="text-button" disabled={loading} onClick={()=>void fetchPage(page.nextOffset!)}>Показать ещё исследования</button>}
  </section>;
}

export type InvestigationDraft={snapshotId:string;graph:SavedGraphState;existing?:Investigation};
export function InvestigationDialog({draft,onClose,onSaved}:{draft:InvestigationDraft;onClose:()=>void;onSaved:(saved:Investigation)=>void}){
  const dialog=useRef<HTMLDialogElement>(null);const [name,setName]=useState(draft.existing?.name??"");const [note,setNote]=useState(draft.existing?.note??"");const [saving,setSaving]=useState(false);const [error,setError]=useState("");
  useEffect(()=>{const element=dialog.current!;element.showModal();return()=>element.close();},[]);
  const save=async(asNew=false)=>{
    if(saving)return;setSaving(true);setError("");
    try{const saved=await invoke<Investigation>("save_investigation",{request:{id:asNew?null:draft.existing?.id??null,expectedRevision:asNew?null:draft.existing?.revision??null,snapshotId:draft.snapshotId,name,note,graph:draft.graph}});onSaved(saved);}
    catch(reason){setError(String(reason));}
    finally{setSaving(false);}
  };
  return <dialog className="investigation-dialog" ref={dialog} onCancel={event=>{event.preventDefault();if(!saving)onClose();}}><form onSubmit={event=>{event.preventDefault();void save();}}><h2>{draft.existing?"Обновить исследование":"Сохранить исследование"}</h2><p>Граф привязан к снимку {draft.snapshotId.slice(0,8)}. Исходный код не анализируется повторно.</p>
    <label>Название<input disabled={saving} autoFocus required maxLength={120} value={name} onChange={event=>setName(event.target.value)} /></label><label>Заметка<textarea disabled={saving} maxLength={16000} rows={6} value={note} onChange={event=>setNote(event.target.value)} /></label>
    {error&&<div role="alert" className="error-banner">{error}</div>}<div className="investigation-actions"><button type="submit" disabled={saving||!name.trim()}>{saving?"Сохраняем…":draft.existing?"Сохранить изменения":"Сохранить"}</button>{draft.existing&&<button type="button" disabled={saving||!name.trim()} onClick={()=>void save(true)}>Сохранить как новое</button>}<button type="button" disabled={saving} onClick={onClose}>Отмена</button></div>
  </form></dialog>;
}
