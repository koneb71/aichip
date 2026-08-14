import { Handle, Position } from "@xyflow/react";
import { PlacedNode } from "../../lib/repoGraph";

/**
 * The two things the map draws: a module you can open, and a file inside an
 * open one.
 *
 * React Flow's `Node<T>` constrains `T` to `Record<string, unknown>`, hence
 * the index signature — an ordinary interface fails at the `Node<T>[]`
 * annotation with an error that points nowhere near the cause.
 */
export interface MapNodeData extends Record<string, unknown> {
  node: PlacedNode;
  /** False when something else is selected and this is not one hop from it. */
  near: boolean;
  /** True when this is the selected node itself. */
  focused: boolean;
  dimmed: boolean;
}

/**
 * One colour per language, from the tint palette the rest of the app uses.
 *
 * Colour carries language, which is a *label*, never a quantity — the size
 * channel is the one carrying a number, and one visual channel per meaning is
 * what keeps a picture readable. A language with no entry gets slate rather
 * than a generated colour, because a colour nobody chose means nothing.
 */
const TINT: Record<string, { bg: string; fg: string }> = {
  rust: { bg: "var(--color-tint-amber)", fg: "var(--color-ink-amber)" },
  typescript: { bg: "var(--color-tint-sky)", fg: "var(--color-ink-sky)" },
  tsx: { bg: "var(--color-tint-indigo)", fg: "var(--color-ink-indigo)" },
  python: { bg: "var(--color-tint-mint)", fg: "var(--color-ink-mint)" },
};
const NEUTRAL = { bg: "var(--color-tint-slate)", fg: "var(--color-ink-slate)" };

export function tintFor(lang: string | null): { bg: string; fg: string } {
  return (lang && TINT[lang]) || NEUTRAL;
}

/** Handles are invisible: this graph has no hand-drawn links, and React Flow
 *  needs somewhere to anchor an edge. */
function Anchors() {
  return (
    <>
      <Handle type="target" position={Position.Left} className="!opacity-0" isConnectable={false} />
      <Handle type="source" position={Position.Right} className="!opacity-0" isConnectable={false} />
    </>
  );
}

/** A whole module: click to open it into its files. */
export function ModuleNode({ data }: { data: MapNodeData }) {
  const { node, focused, dimmed } = data;
  const tint = tintFor(node.lang);
  const size = node.r * 2;
  return (
    <div
      className="flex select-none items-center justify-center rounded-2xl border-2 text-center transition-opacity"
      style={{
        width: size,
        height: size,
        background: node.expanded ? "transparent" : tint.bg,
        borderColor: focused ? "var(--color-accent)" : "var(--color-line)",
        borderStyle: node.expanded ? "dashed" : "solid",
        opacity: dimmed ? 0.25 : 1,
      }}
      title={`${node.label} — ${node.files} files, imported ${node.importedBy}×`}
    >
      <Anchors />
      <div className="px-2">
        <div className="font-mono text-[11px] font-semibold leading-tight">{node.label}</div>
        <div className="mt-0.5 text-[10px] text-ink-dim">
          {node.files} file{node.files === 1 ? "" : "s"}
        </div>
        {!node.expanded && (
          <div className="mt-1 text-[10px] text-ink-dim">
            ← {node.importedBy} · {node.imports} →
          </div>
        )}
      </div>
    </div>
  );
}

/**
 * One file. Size is how many files import it, through a band and a square
 * root so that *area* tracks the count — set width from the raw number and the
 * eye reads the square of it.
 */
export function FileNode({ data }: { data: MapNodeData }) {
  const { node, focused, dimmed } = data;
  const tint = tintFor(node.lang);
  // Clamped to a 6:1 area ratio. This repository's busiest file has 46
  // importers and its quietest has none; unclamped that is one planet and two
  // hundred specks.
  const size = 10 + 14 * Math.sqrt(node.band / 7);
  return (
    <div
      className="rounded-full border transition-opacity"
      style={{
        width: size,
        height: size,
        background: tint.bg,
        borderColor: focused ? "var(--color-accent)" : tint.fg,
        borderWidth: focused ? 2 : 1,
        opacity: dimmed ? 0.2 : 1,
      }}
      title={`${node.id} — imported by ${node.importedBy}, imports ${node.imports}`}
    >
      <Anchors />
    </div>
  );
}

/** Module first, so React Flow's tab order runs modules-then-files. */
export const nodeTypes = { module: ModuleNode, file: FileNode };
