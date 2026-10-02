import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { useTheme } from "../../lib/theme";
import { TERMINAL_THEME } from "../../theme/editorThemes";
import { Button } from "../ui/Button";

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

export default function TerminalPanel({ projectId }: { projectId: string }) {
  const host = useRef<HTMLDivElement | null>(null);
  const [ended, setEnded] = useState(false);
  // Bumped by Restart: tears the whole effect down and opens a fresh shell.
  const [epoch, setEpoch] = useState(0);
  // The terminal matches the editor: both follow the app's theme. Read through
  // a ref at creation so a theme change recolours the live session below rather
  // than tearing the shell down.
  const { theme } = useTheme();
  const themeRef = useRef(theme);
  themeRef.current = theme;
  const termRef = useRef<Terminal | null>(null);

  useEffect(() => {
    if (!host.current) return;
    setEnded(false);

    const term = new Terminal({
      theme: TERMINAL_THEME[themeRef.current],
      fontSize: 12.5,
      fontFamily:
        'ui-monospace, SFMono-Regular, Menlo, Monaco, "Cascadia Mono", monospace',
      cursorBlink: true,
      scrollback: 5000,
    });
    const fit = new FitAddon();
    termRef.current = term;
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
      termRef.current = null;
    };
  }, [projectId, epoch]);

  useEffect(() => {
    if (termRef.current) termRef.current.options.theme = TERMINAL_THEME[theme];
  }, [theme]);

  return (
    <div className="flex h-full min-h-0 flex-col bg-panel">
      <div className="flex items-center gap-2 border-b border-border px-3 py-1.5 text-[11px] text-fg-muted">
        <span className="size-1.5 rounded-full bg-tier-easy" />
        Your shell, in this project's folder. The session ends when you leave this tab.
        {ended && (
          <Button
            size="xs"
            variant="secondary"
            onClick={() => setEpoch((e) => e + 1)}
            className="ml-auto"
          >
            Restart shell
          </Button>
        )}
      </div>
      <div ref={host} className="min-h-0 flex-1 p-2" />
    </div>
  );
}
