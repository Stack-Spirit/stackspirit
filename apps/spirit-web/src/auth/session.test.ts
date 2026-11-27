import { afterEach, describe, expect, it } from "vitest";

import {
  consumeAuthTokenFragment,
  getSessionToken,
  setSessionToken,
} from "./session";

afterEach(() => {
  setSessionToken(null);
  window.history.replaceState(null, "", "/");
});

describe("StackSpirit auth callback fragment", () => {
  it("stores a valid token and removes the fragment from the URL", () => {
    window.history.replaceState(null, "", "/cloud?keep=1#auth_token=auth-token-value");

    const result = consumeAuthTokenFragment(window.location, window.history);

    expect(result).toEqual({ token: "auth-token-value", sawAuthToken: true });
    expect(getSessionToken()).toBe("auth-token-value");
    expect(window.location.pathname).toBe("/cloud");
    expect(window.location.search).toBe("?keep=1");
    expect(window.location.hash).toBe("");
  });

  it("scrubs a malformed callback without storing a token", () => {
    window.history.replaceState(null, "", "/cloud#auth_token=not%20valid");

    const result = consumeAuthTokenFragment(window.location, window.history);

    expect(result).toEqual({ token: null, sawAuthToken: true });
    expect(getSessionToken()).toBeNull();
    expect(window.location.hash).toBe("");
  });

  it("leaves a non-auth fragment alone and ignores malformed setSessionToken values", () => {
    window.history.replaceState(null, "", "/cloud#section");
    expect(consumeAuthTokenFragment(window.location, window.history)).toEqual({
      token: null,
      sawAuthToken: false,
    });
    expect(window.location.hash).toBe("#section");

    setSessionToken("has a space");
    expect(getSessionToken()).toBeNull();
    setSessionToken("auth-token-value");
    expect(getSessionToken()).toBe("auth-token-value");
  });
});
