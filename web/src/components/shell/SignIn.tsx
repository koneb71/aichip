import { useState, type FormEvent, type ReactNode } from "react";
import { api, type User } from "../../lib/api";
import { passwordProblem, usernameProblem } from "../../lib/accounts";
import { Button } from "../ui/Button";
import { Field, Input } from "../ui/Field";
import { cn } from "../ui/cn";

/**
 * The screens shown instead of the app while nobody is signed in, or while
 * an account must replace the temporary password the admin gave it.
 */

function Frame({ title, subtitle, children }: { title: string; subtitle?: ReactNode; children: ReactNode }) {
  return (
    <main className="grid min-h-screen place-items-center bg-bg px-4 py-10 text-fg">
      <div className="w-full max-w-sm">
        <div className="mb-6 text-center">
          <div className="text-[13px] font-semibold tracking-tight text-fg-muted">Eren</div>
          <h1 className="mt-1 text-xl font-semibold tracking-tight">{title}</h1>
          {subtitle && <p className="mt-1.5 text-[13px] leading-relaxed text-fg-muted">{subtitle}</p>}
        </div>
        <div className="rounded-xl border border-border bg-panel p-5 shadow-[var(--shadow-sm)]">{children}</div>
      </div>
    </main>
  );
}

function Problem({ children }: { children: ReactNode }) {
  if (!children) return null;
  return (
    <p role="alert" className="rounded-md bg-danger-subtle px-2.5 py-2 text-xs leading-relaxed text-danger-fg">
      {children}
    </p>
  );
}

/** What the server said, minus the noise of an empty body. */
function said(e: unknown): string {
  const text = e instanceof Error ? e.message.trim() : "";
  return text || "Something went wrong. Try again.";
}

export function SignInScreen({ signupOpen, onSignedIn }: { signupOpen: boolean; onSignedIn: () => void }) {
  const [mode, setMode] = useState<"in" | "up">("in");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [again, setAgain] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const signingUp = mode === "up";

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const local = signingUp ? usernameProblem(username) ?? passwordProblem(password, again) : null;
    if (local) {
      setProblem(local);
      return;
    }
    setBusy(true);
    setProblem(null);
    try {
      if (signingUp) await api.signup(username, password);
      else await api.login(username, password);
      onSignedIn();
    } catch (err) {
      setProblem(said(err));
      setBusy(false);
    }
  };

  const switchTo = (next: "in" | "up") => {
    setMode(next);
    setProblem(null);
    setAgain("");
  };

  return (
    <Frame
      title={signingUp ? "Create your account" : "Sign in"}
      subtitle={signingUp ? "You start with a workspace of your own." : undefined}
    >
      {signupOpen && (
        <div role="tablist" aria-label="Sign in or sign up" className="mb-4 grid grid-cols-2 gap-1 rounded-lg bg-panel-2 p-1">
          {(["in", "up"] as const).map((m) => (
            <button
              key={m}
              type="button"
              role="tab"
              aria-selected={mode === m}
              onClick={() => switchTo(m)}
              className={cn(
                "ring-focus h-7 rounded-md text-[13px] font-medium transition-colors duration-[var(--dur-fast)]",
                mode === m ? "bg-raised text-fg shadow-[var(--shadow-xs)]" : "text-fg-muted hover:text-fg",
              )}
            >
              {m === "in" ? "Sign in" : "Create account"}
            </button>
          ))}
        </div>
      )}
      <form className="flex flex-col gap-3.5" onSubmit={submit} noValidate>
        <Field label="Username" hint={signingUp ? "Letters, digits and . _ - (3 to 32)." : undefined}>
          {(id) => (
            <Input
              id={id}
              autoFocus
              autoComplete="username"
              autoCapitalize="none"
              spellCheck={false}
              value={username}
              onChange={(e) => setUsername(e.target.value)}
            />
          )}
        </Field>
        <Field label="Password" hint={signingUp ? "At least 10 characters." : undefined}>
          {(id) => (
            <Input
              id={id}
              type="password"
              autoComplete={signingUp ? "new-password" : "current-password"}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          )}
        </Field>
        {signingUp && (
          <Field label="Password again">
            {(id) => (
              <Input id={id} type="password" autoComplete="new-password" value={again} onChange={(e) => setAgain(e.target.value)} />
            )}
          </Field>
        )}
        <Problem>{problem}</Problem>
        <Button type="submit" variant="primary" disabled={busy || !username || !password} className="mt-1 w-full justify-center">
          {busy ? (signingUp ? "Creating…" : "Signing in…") : signingUp ? "Create account" : "Sign in"}
        </Button>
      </form>
      {!signupOpen && (
        <p className="mt-4 text-center text-xs leading-relaxed text-fg-muted">
          No account? Ask the admin — sign-up is closed.
        </p>
      )}
    </Frame>
  );
}

/** The form for choosing a new password: forced after an admin's reset, or opened from the account menu. */
export function ChangePasswordForm({ onChanged, currentLabel = "Current password" }: { onChanged: () => void; currentLabel?: string }) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [again, setAgain] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const local = passwordProblem(next, again);
    if (local) {
      setProblem(local);
      return;
    }
    setBusy(true);
    setProblem(null);
    try {
      await api.changePassword(current, next);
      onChanged();
    } catch (err) {
      setProblem(said(err));
      setBusy(false);
    }
  };

  return (
    <form className="flex flex-col gap-3.5" onSubmit={submit} noValidate>
      <Field label={currentLabel}>
        {(id) => (
          <Input id={id} type="password" autoFocus autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} />
        )}
      </Field>
      <Field label="New password" hint="At least 10 characters.">
        {(id) => <Input id={id} type="password" autoComplete="new-password" value={next} onChange={(e) => setNext(e.target.value)} />}
      </Field>
      <Field label="New password again">
        {(id) => <Input id={id} type="password" autoComplete="new-password" value={again} onChange={(e) => setAgain(e.target.value)} />}
      </Field>
      <Problem>{problem}</Problem>
      <Button type="submit" variant="primary" disabled={busy || !current || !next} className="mt-1 w-full justify-center">
        {busy ? "Saving…" : "Change password"}
      </Button>
    </form>
  );
}

export function ChangePasswordScreen({
  user,
  onChanged,
  onSignOut,
}: {
  user: User;
  onChanged: () => void;
  onSignOut: () => void;
}) {
  return (
    <Frame
      title="Choose a new password"
      subtitle={
        <>
          The admin reset the password for <span className="font-medium text-fg">{user.username}</span>. Use the temporary one once, here.
        </>
      }
    >
      <ChangePasswordForm onChanged={onChanged} currentLabel="Temporary password" />
      <button type="button" onClick={onSignOut} className="ring-focus mt-4 w-full rounded-md py-1 text-center text-xs text-fg-muted hover:text-fg">
        Sign out
      </button>
    </Frame>
  );
}
