import { useState } from "react";
import { motion } from "framer-motion";

/**
 * A page's icon.
 *
 * A grid rather than an emoji-picker dependency: forty-eight is enough to make
 * a tree scannable, and scanning is the whole point — the icon is how you find
 * the runbook among forty rows of near-identical text.
 */
const ICONS = [
  "▦", "📄", "📘", "📕", "📗", "🗂", "🧭", "🚀",
  "🛠", "⚙️", "🔧", "🧪", "🐛", "🔐", "🔑", "🛡",
  "📦", "🗄", "💾", "🌐", "☁️", "⚡", "🔥", "💡",
  "📈", "📊", "🧮", "🗓", "⏱", "🔔", "📌", "🏷",
  "✅", "⚠️", "❓", "💬", "📝", "🖇", "🔗", "🧵",
  "🏗", "🧱", "🪝", "🧩", "🎯", "🗺", "🚦", "🧯",
];

export function IconPicker({
  value,
  onChange,
}: {
  value: string;
  onChange: (icon: string) => void;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        title="Change the icon"
        className="rounded-lg px-1.5 py-0.5 text-3xl leading-none hover:bg-panel-2"
      >
        {value || "▦"}
      </button>
      {open && (
        <motion.div
          initial={{ opacity: 0, y: -4 }}
          animate={{ opacity: 1, y: 0 }}
          className="card-shadow absolute left-0 top-full z-20 mt-1 w-64 rounded-xl border border-line bg-panel p-2"
        >
          <div className="grid grid-cols-8 gap-0.5">
            {ICONS.map((icon) => (
              <button
                key={icon}
                onClick={() => {
                  onChange(icon);
                  setOpen(false);
                }}
                className="rounded-lg py-1 text-lg hover:bg-panel-2"
              >
                {icon}
              </button>
            ))}
          </div>
          <button
            onClick={() => {
              onChange("");
              setOpen(false);
            }}
            className="mt-1 w-full rounded-lg px-2 py-1 text-left text-xs text-ink-dim hover:bg-panel-2"
          >
            Remove
          </button>
        </motion.div>
      )}
    </div>
  );
}
