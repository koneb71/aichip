import { useEffect, useLayoutEffect, useRef, useState } from "react";

/**
 * Pick a table size by dragging over a grid.
 *
 * The shape every editor uses, for a good reason: choosing "4 × 3" is a spatial
 * decision, and a grid you sweep answers it in one gesture. Inserting a fixed
 * 3×3 and making people add the rest afterwards turns one action into four.
 *
 * The grid grows as you approach its edge, so it starts small and still reaches
 * a large table without a scrollbar — Word's behaviour, and the one people
 * already have in their fingers.
 */
const START_COLS = 5;
const START_ROWS = 5;
const MAX_COLS = 10;
const MAX_ROWS = 8;
/** Cell + gap, in px, matching the `h-4 w-4` and `gap-[3px]` below. */
const CELL = 16 + 3;
/** How wide the panel gets once the grid is fully grown, plus its padding. */
const MAX_WIDTH = MAX_COLS * CELL + 16;

export function TableSizePicker({
  onPick,
}: {
  onPick: (rows: number, cols: number, withHeaderRow: boolean) => void;
}) {
  const [open, setOpen] = useState(false);
  const [cols, setCols] = useState(START_COLS);
  const [rows, setRows] = useState(START_ROWS);
  // 0 means "nothing hovered yet".
  const [hover, setHover] = useState({ r: 0, c: 0 });
  const [header, setHeader] = useState(true);
  const boxRef = useRef<HTMLDivElement>(null);
  const popRef = useRef<HTMLDivElement>(null);
  const [shift, setShift] = useState(0);

  // Reset every time it opens, so a previous sweep doesn't preselect a size
  // the next person didn't choose.
  useEffect(() => {
    if (!open) return;
    setCols(START_COLS);
    setRows(START_ROWS);
    setHover({ r: 0, c: 0 });
    boxRef.current?.focus();
  }, [open]);

  // Keep the popover on screen — the table button sits near the right of a
  // toolbar that wraps, so a left-anchored panel gets clipped by the window.
  //
  // Computed ONCE, from the width the grid will have when fully grown, and
  // deliberately not re-run as it grows. Re-clamping mid-sweep slides the panel
  // sideways under a stationary cursor, so the cell you release on is not the
  // one you were looking at: hovering "4 × 5" inserted a 4 × 6 table. Reserving
  // the full width up front means growth only ever fills cells in, and nothing
  // moves.
  useLayoutEffect(() => {
    if (!open) return;
    const el = popRef.current;
    if (!el) return;
    const left = el.getBoundingClientRect().left - shift;
    const overflow = left + MAX_WIDTH - (window.innerWidth - 8);
    setShift(overflow > 0 ? -overflow : 0);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const move = (r: number, c: number) => {
    setHover({ r, c });
    // Grow when the pointer reaches the far edge, so the grid is never a wall.
    if (c >= cols && cols < MAX_COLS) setCols(Math.min(c + 1, MAX_COLS));
    if (r >= rows && rows < MAX_ROWS) setRows(Math.min(r + 1, MAX_ROWS));
  };

  const pick = (r: number, c: number) => {
    if (r < 1 || c < 1) return;
    onPick(r, c, header);
    setOpen(false);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const { r, c } = hover;
    const step = (dr: number, dc: number) => {
      e.preventDefault();
      move(Math.min(Math.max(r + dr, 1), MAX_ROWS), Math.min(Math.max(c + dc, 1), MAX_COLS));
    };
    if (e.key === "ArrowRight") return step(r === 0 ? 1 : 0, 1);
    if (e.key === "ArrowLeft") return step(0, -1);
    if (e.key === "ArrowDown") return step(1, c === 0 ? 1 : 0);
    if (e.key === "ArrowUp") return step(-1, 0);
    if (e.key === "Enter") {
      e.preventDefault();
      pick(r || 1, c || 1);
    }
    if (e.key === "Escape") setOpen(false);
  };

  return (
    <div className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        title="Insert a table"
        className={`rounded-md px-2 py-1 text-xs ${
          open ? "bg-accent/10 text-accent" : "text-ink-dim hover:bg-panel-2 hover:text-ink"
        }`}
      >
        ▦
      </button>

      {open && (
        <>
          <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} />
          <div
            ref={popRef}
            style={{ transform: `translateX(${shift}px)` }}
            className="card-shadow absolute left-0 top-full z-20 mt-1 rounded-xl border border-line bg-panel p-2"
          >
            <div
              ref={boxRef}
              tabIndex={0}
              role="grid"
              aria-label="Table size"
              onKeyDown={onKeyDown}
              onMouseLeave={() => setHover({ r: 0, c: 0 })}
              className="grid gap-[3px] outline-none"
              style={{ gridTemplateColumns: `repeat(${cols}, 1rem)` }}
            >
              {Array.from({ length: rows * cols }, (_, i) => {
                const r = Math.floor(i / cols) + 1;
                const c = (i % cols) + 1;
                const on = r <= hover.r && c <= hover.c;
                return (
                  <button
                    key={i}
                    type="button"
                    tabIndex={-1}
                    aria-label={`${r} by ${c}`}
                    onMouseEnter={() => move(r, c)}
                    onClick={() => pick(r, c)}
                    className={`h-4 w-4 rounded-[3px] border ${
                      on ? "border-accent bg-accent/25" : "border-line bg-panel-2"
                    }`}
                  />
                );
              })}
            </div>

            <div className="mt-2 text-center text-[11px] text-ink-dim">
              {hover.r && hover.c ? (
                <span className="font-medium text-ink">
                  {hover.r} × {hover.c}
                </span>
              ) : (
                "Drag to size"
              )}
            </div>

            <label className="mt-1.5 flex cursor-pointer items-center gap-1.5 border-t border-line pt-1.5 text-[11px] text-ink-dim">
              <input
                type="checkbox"
                checked={header}
                onChange={(e) => setHeader(e.target.checked)}
                className="accent-[var(--color-accent)]"
              />
              Header row
            </label>
          </div>
        </>
      )}
    </div>
  );
}
