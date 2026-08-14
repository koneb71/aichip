import { useState } from "react";
import { motion } from "framer-motion";
import { OpenQuestion } from "../../lib/api";

/**
 * The assistant asking, instead of guessing.
 *
 * Options rather than a prose question, because a closed set is unambiguous
 * in both directions: the person answers with a click, and the assistant gets
 * back a label it chose the wording of rather than a sentence it has to
 * interpret.
 *
 * There is always a way out that is not one of the options — the composer is
 * right below, and the card says so. A question that can only be answered its
 * own way is a form, and forms are where people pick the least wrong box and
 * the assistant proceeds confidently on it.
 */
export function QuestionCard({
  open,
  onAnswer,
  busy,
}: {
  open: OpenQuestion;
  onAnswer: (answers: string[][]) => void;
  /** A turn is running; answering would be refused server-side anyway. */
  busy?: boolean;
}) {
  // One set of picks per question, by index.
  const [picked, setPicked] = useState<string[][]>(() => open.questions.map(() => []));

  // One question, one choice, no second press — clicking the option *is* the
  // answer, the way it is in Claude Code. Requiring a Send after a
  // single-choice click is a step with no decision in it, and a button that
  // does nothing the click did not already say makes people wonder whether
  // the click registered.
  //
  // The Send button stays for everything else: with several questions, or a
  // multi-select, there is no moment the UI can know you have finished
  // choosing.
  const oneShot = open.questions.length === 1 && !open.questions[0].multiSelect;

  const toggle = (qi: number, label: string, multi: boolean) => {
    if (oneShot) {
      onAnswer([[label]]);
      return;
    }
    setPicked((prev) =>
      prev.map((set, i) => {
        if (i !== qi) return set;
        if (!multi) return set.includes(label) ? [] : [label];
        return set.includes(label) ? set.filter((l) => l !== label) : [...set, label];
      }),
    );
  };

  const answered = picked.filter((p) => p.length > 0).length;
  const all = open.questions.length;

  return (
    <motion.div
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.15, ease: "easeOut" }}
      className="self-start w-full max-w-[85%] rounded-2xl border border-accent/40 bg-panel px-3 py-2.5"
    >
      <div className="mb-2 text-[10px] font-medium uppercase tracking-wide text-accent">
        ? Before going further
      </div>

      <div className="space-y-3">
        {open.questions.map((q, qi) => (
          <div key={qi}>
            <div className="flex items-baseline gap-1.5">
              {q.header && (
                <span className="shrink-0 rounded-full bg-panel-2 px-1.5 text-[10px] text-ink-dim">
                  {q.header}
                </span>
              )}
              <span className="text-sm">{q.question}</span>
            </div>
            <div className="mt-1.5 flex flex-wrap gap-1.5">
              {q.options.map((o) => {
                const on = picked[qi]?.includes(o.label);
                return (
                  <button
                    key={o.label}
                    onClick={() => toggle(qi, o.label, !!q.multiSelect)}
                    disabled={busy}
                    title={o.description}
                    className={`ring-focus rounded-lg border px-2.5 py-1 text-left text-xs transition-colors disabled:opacity-50 ${
                      on
                        ? "border-accent bg-accent/10 font-medium text-accent"
                        : "border-line hover:border-accent/50 hover:bg-panel-2"
                    }`}
                  >
                    <span className="block">{o.label}</span>
                    {o.description && (
                      <span className="block max-w-56 truncate text-[10px] font-normal text-ink-dim">
                        {o.description}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          </div>
        ))}
      </div>

      <div className="mt-2.5 flex items-center gap-2 border-t border-accent/20 pt-2">
        {!oneShot && (
          <button
            onClick={() => onAnswer(picked)}
            disabled={busy || answered === 0}
            className="ring-focus rounded-lg bg-accent px-2.5 py-1 text-[11px] text-white disabled:opacity-40"
          >
            {all > 1 ? `Send ${answered} of ${all}` : "Send"}
          </button>
        )}
        {/* The escape hatch, said out loud. Without it the card reads as the
            only way to reply, and somebody picks the least wrong option. */}
        <span className="text-[10px] text-ink-dim">
          {oneShot
            ? "Pick one — or answer in your own words below, that works too"
            : "or answer in your own words below — that works too"}
        </span>
      </div>
    </motion.div>
  );
}
