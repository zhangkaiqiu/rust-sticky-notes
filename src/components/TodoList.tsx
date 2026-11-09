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
        <span className="note-label text-[0.625em] font-semibold uppercase tracking-[0.14em]">
          待办
          {todos.length > 0 && (
            <span className="note-faint ml-1 font-normal tracking-normal">
              {doneCount}/{todos.length}
            </span>
          )}
        </span>
        {doneCount > 0 && (
          <button
            type="button"
            onClick={onClearDone}
            className="note-faint note-clear-hover rounded px-1 text-[0.625em] transition-colors"
          >
            清除已完成
          </button>
        )}
      </header>

      <ul className="note-scroll min-h-0 flex-1 overflow-y-auto px-1.5">
        {todos.map((item) => (
          <li
            key={item.id}
            className="note-hover group flex items-center gap-2 rounded-md px-2 py-[3px]"
          >
            <button
              type="button"
              role="checkbox"
              aria-checked={item.done}
              aria-label={item.text}
              onClick={() => onToggle(item.id)}
              className={`flex h-[0.9375em] w-[0.9375em] shrink-0 items-center justify-center rounded-[5px] border transition-colors ${
                item.done
                  ? "note-accent-bg border-transparent"
                  : "note-checkbox bg-transparent"
              }`}
            >
              <CheckIcon
                className={`h-[0.5625em] w-[0.5625em] ${
                  item.done ? "" : "text-transparent"
                }`}
              />
            </button>

            <input
              value={item.text}
              onChange={(e) => onEdit(item.id, e.target.value)}
              className={`min-w-0 flex-1 bg-transparent text-[0.78125em] leading-snug outline-none ${
                item.done ? "note-muted-soft line-through" : "note-text"
              }`}
            />

            <button
              type="button"
              title="删除"
              onClick={() => onRemove(item.id)}
              className="note-faint note-danger-hover shrink-0 rounded p-0.5 opacity-0 transition-opacity focus-visible:opacity-100 group-hover:opacity-100"
            >
              <DeleteIcon className="h-[0.75em] w-[0.75em]" />
            </button>
          </li>
        ))}
      </ul>

      <form onSubmit={submit} className="shrink-0 px-3 pb-2 pt-1">
        <div className="note-input-bar flex items-center gap-1.5 rounded-lg px-2 py-1">
          <span className="note-faint text-[0.75em] leading-none">＋</span>
          <input
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder="添加待办，回车确认"
            className="note-placeholder w-full bg-transparent text-[0.78125em] outline-none"
          />
        </div>
      </form>
    </section>
  );
}
