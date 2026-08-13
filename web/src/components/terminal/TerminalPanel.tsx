import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

/**
 * A real shell in the project's folder — the user's own login shell, over a
 * WebSocket the dashboard's Host/Origin guard protects like everything else.
 *
 * The session lives exactly as long as this panel: switching tabs is closing
 * the terminal window. That is stated in the UI rather than smoothed over,
 * because "my shell vanished" and "my shell was never meant to persist" feel
 * completely different when you know which one is true.
 *
 * This file is the only importer of xterm, and it is loaded through
 * `React.lazy` — the same rule Monaco follows, so people who never open the
 * tab never download the emulator.
 */

/** The IDE palette the Files tab established — the terminal matches it. */
const THEME = {
  background: "#1e1e1e",
  foreground: "#cccccc",
  cursor: "#cccccc",
  selectionBackground: "#264f78",
  black: "#000000",
  red: "#f48771",
  green: "#89d185",
  yellow: "#e2c08d",
  blue: "#569cd6",
  magenta: "#c586c0",
  cyan: "#4ec9b0",
  white: "#cccccc",
  brightBlack: "#6e7681",
  brightRed: "#f48771",
  brightGreen: "#89d185",
  brightYellow: "#e2c08d",
  brightBlue: "#569cd6",
  brightMagenta: "#c586c0",
  brightCyan: "#4ec9b0",
  brightWhite: "#ffffff",
};

export default function TerminalPanel({ projectId }: { projectId: string }) {
  const host = useRef<HTMLDivElement | null>(null);
  const [ended, setEnded] = useState(false);
  // Bumped by Restart: tears the whole effect down and opens a fresh shell.
  const [epoch, setEpoch] = useState(0);

  useEffect(() => {
    if (!host.current) return;
    setEnded(false);

    const term = new Terminal({
      theme: THEME,
      fontSize: 12.5,
      fontFamily:
        'ui-monospace, SFMono-Regular, Menlo, Monaco, "Cascadia Mono", monospace',
      cursorBlink: true,
      scrollback: 5000,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host.current);
    fit.fit();

    const proto = location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(`${proto}://${location.host}/ws/terminal/${projectId}`);
    ws.binaryType = "arraybuffer";

    const sendResize = () => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ resize: { cols: term.cols, rows: term.rows } }));
      }
    };

    ws.onopen = () => {
      sendResize();
      term.focus();
    };
    ws.onmessage = (e) => {
      if (typeof e.data === "string") term.write(e.data);
      else term.write(new Uint8Array(e.data));
    };
    ws.onclose = () => setEnded(true);
    ws.onerror = () => setEnded(true);

    const data = term.onData((d) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(new TextEncoder().encode(d));
      }
    });

    // Refit on any size change, and tell the pty — a shell whose idea of its
    // own width is stale wraps every long line in the wrong place.
    const resize = new ResizeObserver(() => {
      fit.fit();
      sendResize();
    });
    resize.observe(host.current);

    return () => {
      resize.disconnect();
      data.dispose();
      ws.close();
      term.dispose();
    };
  }, [projectId, epoch]);

  return (
    <div className="flex h-full min-h-0 flex-col bg-[#1e1e1e]">
      <div className="flex items-center gap-2 border-b border-[#3c3c3c] px-3 py-1.5 text-[11px] text-[#8c8c8c]">
        <span className="size-1.5 rounded-full bg-tier-easy" />
        Your shell, in this project's folder. The session ends when you leave this tab.
        {ended && (
          <button
            onClick={() => setEpoch((e) => e + 1)}
            className="ml-auto rounded border border-[#3c3c3c] px-2 py-0.5 text-[#cccccc] hover:bg-[#2a2d2e]"
          >
            Restart shell
          </button>
        )}
      </div>
      <div ref={host} className="min-h-0 flex-1 p-2" />
    </div>
  );
}
