/**
 * The dropdown that `@` and `/` share.
 *
 * One renderer for both, so they behave identically — the arrow keys, the
 * highlight, the way Escape backs out. Two near-identical popups that disagree
 * about which key selects is the kind of small inconsistency people never
 * report and always notice.
 *
 * Hand-rolled rather than a popover dependency: it is one absolutely positioned
 * list, and this whole editor change existed to remove a dependency.
 */
export interface MenuItem {
  id: string;
  label: string;
  hint?: string;
}

export function menuRenderer<T extends MenuItem>() {
  let el: HTMLDivElement | null = null;
  let items: T[] = [];
  let selected = 0;
  let command: ((item: T) => void) | null = null;

  const draw = () => {
    if (!el) return;
    el.innerHTML = "";
    if (!items.length) {
      el.style.display = "none";
      return;
    }
    el.style.display = "block";
    items.forEach((item, i) => {
      const row = document.createElement("button");
      row.type = "button";
      row.className =
        "flex w-full items-baseline gap-2 rounded-lg px-2 py-1.5 text-left " +
        (i === selected ? "bg-accent/10 text-accent" : "hover:bg-panel-2");
      const name = document.createElement("span");
      name.className = "truncate text-xs font-medium";
      name.textContent = item.label;
      row.appendChild(name);
      if (item.hint) {
        const hint = document.createElement("span");
        hint.className = "ml-auto shrink-0 text-[10px] text-ink-dim";
        hint.textContent = item.hint;
        row.appendChild(hint);
      }
      // `mousedown`, not `click`: the editor loses focus on mousedown and the
      // suggestion plugin tears this menu down before a click could land.
      row.addEventListener("mousedown", (e) => {
        e.preventDefault();
        command?.(item);
      });
      el!.appendChild(row);
      if (i === selected) row.scrollIntoView({ block: "nearest" });
    });
  };

  const place = (rect: DOMRect | null | undefined) => {
    if (!el || !rect) return;
    // Flip above the caret when there is no room below, so the menu is never
    // half off the bottom of the window.
    const below = window.innerHeight - rect.bottom;
    const height = Math.min(el.scrollHeight || 240, 240);
    el.style.left = `${Math.min(rect.left, window.innerWidth - 280)}px`;
    if (below < height + 16) {
      el.style.top = `${Math.max(8, rect.top - height - 6)}px`;
    } else {
      el.style.top = `${rect.bottom + 6}px`;
    }
  };

  return {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    onStart: (props: any) => {
      el = document.createElement("div");
      el.className =
        "card-shadow fixed z-50 max-h-60 w-64 overflow-y-auto rounded-xl border border-line bg-panel p-1";
      document.body.appendChild(el);
      items = props.items;
      selected = 0;
      command = (item: T) => props.command(item);
      draw();
      place(props.clientRect?.());
    },
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    onUpdate: (props: any) => {
      items = props.items;
      selected = 0;
      command = (item: T) => props.command(item);
      draw();
      place(props.clientRect?.());
    },
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    onKeyDown: (props: any) => {
      const key = props.event.key;
      if (key === "Escape") {
        el?.remove();
        el = null;
        return true;
      }
      if (!items.length) return false;
      if (key === "ArrowDown") {
        selected = (selected + 1) % items.length;
        draw();
        return true;
      }
      if (key === "ArrowUp") {
        selected = (selected - 1 + items.length) % items.length;
        draw();
        return true;
      }
      if (key === "Enter" || key === "Tab") {
        command?.(items[selected]);
        return true;
      }
      return false;
    },
    onExit: () => {
      el?.remove();
      el = null;
    },
  };
}
