/**
 * The Web/PWA StackSpirit Account session token, kept in memory only.
 *
 * The OAuth callback arrives as `#auth_token=<token>`. It is parsed once, stored
 * in this module's memory and removed from the address bar before any render can
 * read it. Like the shared cloud client, the token is never written to browser
 * storage, a cookie, a log, an error or a view model.
 */

let currentToken: string | null = null;
const listeners = new Set<(token: string | null) => void>();

/** A bearer token is opaque and must never contain whitespace. */
const TOKEN_PATTERN = /^[A-Za-z0-9._~+/=-]{8,4096}$/;

export function getSessionToken(): string | null {
  return currentToken;
}

export function setSessionToken(token: string | null): void {
  const next = token && TOKEN_PATTERN.test(token) ? token : null;
  if (next === currentToken) return;
  currentToken = next;
  for (const listener of listeners) listener(currentToken);
}

export function subscribeSessionToken(
  listener: (token: string | null) => void,
): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export interface AuthFragmentLocation {
  hash: string;
  pathname: string;
  search: string;
}

export interface AuthFragmentResult {
  /** The valid token when the fragment carried one. */
  token: string | null;
  /** True when an `auth_token` key was present, valid or not. */
  sawAuthToken: boolean;
}

/**
 * Reads the OAuth callback fragment, scrubs it from the URL and stores a valid
 * token. The fragment is removed even when the value is malformed so a bad
 * callback can never stay in the address bar or browser history.
 */
export function consumeAuthTokenFragment(
  location: AuthFragmentLocation,
  history: Pick<History, "replaceState">,
): AuthFragmentResult {
  const raw = location.hash.startsWith("#") ? location.hash.slice(1) : location.hash;
  if (!raw) return { token: null, sawAuthToken: false };
  const params = new URLSearchParams(raw);
  if (!params.has("auth_token")) return { token: null, sawAuthToken: false };
  const value = params.get("auth_token") ?? "";
  history.replaceState(null, "", `${location.pathname}${location.search}`);
  if (!TOKEN_PATTERN.test(value)) {
    return { token: null, sawAuthToken: true };
  }
  setSessionToken(value);
  return { token: value, sawAuthToken: true };
}
