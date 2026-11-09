import { useState } from "react";
import type { FormEvent } from "react";
import type { TodoItem } from "../types";
import { CheckIcon, DeleteIcon } from "./icons";

type Props = {
  todos: TodoItem[];
  onAdd: (text: string) => void;
  onToggle: (id: string) => void;
  onEdit: (id: string, next: string) => void;
  onRemove: (id: string) => void;
  onClearDone: () => void;
};

export default function TodoList({
  todos,
  onAdd,
  onToggle,
  onEdit,
  onRemove,
  onClearDone,
}: Props) {
  const [draft, setDraft] = useState("");
  const doneCount = todos.reduce((n, t) => (t.done ? n + 1 : n), 0);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!draft.trim()) return;
    onAdd(draft);
    setDraft("");
  };

  return (
    <section className="flex max-h-[44%] shrink-0 flex-col pt-1.5">
      <header className="flex shrink-0 items-center justify-between px-3.5 pb-1">
        <span className="text-[10px] font-semibold uppercase tracking-[0.14em] text-amber-900/45">
          待办
          {todos.length > 0 && (
            <span className="ml-1 font-normal tracking-normal text-amber-900/35">
              {doneCount}/{todos.length}
            </span>
          )}
        </span>
        {doneCount > 0 && (
          <button
            type="button"
            onClick={onClearDone}
            className="rounded px-1 text-[10px] text-amber-900/40 transition-colors hover:text-red-600/80"
          >
            清除已完成
          </button>
        )}
      </header>

      <ul className="min-h-0 flex-1 overflow-y-auto px-1.5">
        {todos.map((item) => (
          <li
            key={item.id}
            className="group flex items-center gap-2 rounded-md px-2 py-[3px] hover:bg-amber-900/[0.06]"
          >
            <button
              type="button"
              role="checkbox"
              aria-checked={item.done}
              aria-label={item.text}
              onClick={() => onToggle(item.id)}
              className={`flex h-[15px] w-[15px] shrink-0 items-center justify-center rounded-[5px] border transition-colors ${
                item.done
                  ? "border-amber-500 bg-amber-500 text-white"
                  : "border-amber-900/25 bg-white/50 text-transparent hover:border-amber-500/70"
              }`}
            >
              <CheckIcon className="h-[9px] w-[9px]" />
            </button>

            <input
              value={item.text}
              onChange={(e) => onEdit(item.id, e.target.value)}
              className={`min-w-0 flex-1 bg-transparent text-[12.5px] leading-snug outline-none ${
                item.done
                  ? "text-stone-500/70 line-through decoration-stone-400/60"
                  : "text-stone-800"
              }`}
            />

            <button
              type="button"
              title="删除"
              onClick={() => onRemove(item.id)}
              className="shrink-0 rounded p-0.5 text-amber-900/35 opacity-0 transition-opacity hover:text-red-600/80 focus-visible:opacity-100 group-hover:opacity-100"
            >
              <DeleteIcon className="h-3 w-3" />
            </button>
          </li>
        ))}
      </ul>

      <form onSubmit={submit} className="shrink-0 px-3 pb-2 pt-1">
        <div className="flex items-center gap-1.5 rounded-lg bg-amber-900/[0.05] px-2 py-1 transition-colors focus-within:bg-amber-900/[0.08]">
          <span className="text-[12px] leading-none text-amber-900/40">＋</span>
          <input
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder="添加待办，回车确认"
            className="w-full bg-transparent text-[12.5px] text-stone-800 outline-none placeholder:text-stone-500/45"
          />
        </div>
      </form>
    </section>
  );
}
