import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { api, type AuthStatus, type User } from "./api";
import { SIGNED_OUT_EVENT } from "./accounts";
import { SignInScreen, ChangePasswordScreen } from "../components/shell/SignIn";

/**
 * Who is signed in, decided before anything else mounts.
 *
 * Above the workspace, activity and inbox providers on purpose: all three
 * fetch the moment they mount, and with accounts on every one of those calls
 * would be a 401 before anybody had signed in. With accounts off (no admin
 * yet) this renders the app straight away and nothing changes.
 */

interface AuthValue {
  /** Accounts are on: an admin exists. */
  accounts: boolean;
  signupOpen: boolean;
  user: User | null;
  /** May change the machine's settings: the admin, or anyone while accounts are off. */
  isAdmin: boolean;
  signOut: () => Promise<void>;
  refresh: () => Promise<void>;
}

const AuthContext = createContext<AuthValue>({
  accounts: false,
  signupOpen: false,
  user: null,
  isAdmin: true,
  signOut: async () => {},
  refresh: async () => {},
});

export function useAuth() {
  return useContext(AuthContext);
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AuthStatus | null>(null);
  const [failed, setFailed] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setStatus(await api.authStatus());
      setFailed(false);
    } catch {
      setFailed(true);
    }
  }, []);

  useEffect(() => {
    void refresh();
    // A 401 anywhere means the session is gone: ask again, which brings the
    // sign-in screen back without a reload.
    const onSignedOut = () => void refresh();
    window.addEventListener(SIGNED_OUT_EVENT, onSignedOut);
    return () => window.removeEventListener(SIGNED_OUT_EVENT, onSignedOut);
  }, [refresh]);

  const signOut = useCallback(async () => {
    try {
      await api.logout();
    } finally {
      // Whatever the providers below hold belongs to the account that left.
      window.location.assign("/");
    }
  }, []);

  if (!status) {
    return (
      <div className="grid h-screen place-items-center bg-bg text-sm text-fg-muted">
        {failed ? (
          <button type="button" className="ring-focus rounded-md px-3 py-1.5 hover:bg-panel-2" onClick={() => void refresh()}>
            Could not reach Eren — try again
          </button>
        ) : (
          "Loading…"
        )}
      </div>
    );
  }

  if (status.accounts && !status.user) {
    // A full reload after signing in, so every provider starts as this account.
    return <SignInScreen signupOpen={status.signup} onSignedIn={() => window.location.reload()} />;
  }

  if (status.user?.mustChangePassword) {
    return <ChangePasswordScreen user={status.user} onChanged={() => window.location.reload()} onSignOut={signOut} />;
  }

  const value: AuthValue = {
    accounts: status.accounts,
    signupOpen: status.signup,
    user: status.user,
    isAdmin: !status.accounts || !!status.user?.isAdmin,
    signOut,
    refresh,
  };
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}
