// Imported first by main.tsx, so settings saved under the old name are in
// place before any module or component reads one. Storage can throw (a
// private window, blocked site data); the dashboard works without it.
import { adoptLegacyStorage } from "./brand";

try {
  adoptLegacyStorage(window.localStorage);
} catch {
  // Nothing to move, or nowhere to move it.
}
