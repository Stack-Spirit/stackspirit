import { beforeEach, describe, expect, it } from "vitest";

import { createBridge, setupRedirectUrl } from "./create";

const LOCATION = {
  search: "",
  pathname: "/app",
  hash: "",
  origin: "http://web.test",
};

beforeEach(() => {
  window.sessionStorage.clear();
  window.history.replaceState(null, "", "/app");
});

describe("bridge factory", () => {
  it("returns a preview bridge for VITE_SPIRIT_MOCK=1", () => {
    const boot = createBridge(
      { mock: "1", scenario: "web-read-only", daemonUrl: undefined },
      LOCATION,
    );
    expect(boot.bridge?.preview).toBe(true);
    expect(boot.bridge?.kind).toBe("mock");
    expect(boot.setup).toBe(false);
    boot.bridge?.dispose?.();
  });

  it("asks for an address when neither the query nor the environment has one", () => {
    const boot = createBridge(
      { mock: "0", scenario: undefined, daemonUrl: undefined },
      LOCATION,
    );
    expect(boot.bridge).toBeNull();
    expect(boot.setup).toBe(true);
    expect(boot.error).toBeNull();
  });

  it("rejects a different API origin with the same-origin copy", () => {
    const boot = createBridge(
      { mock: "0", scenario: undefined, daemonUrl: "http://127.0.0.1:5174" },
      LOCATION,
    );
    expect(boot.bridge).toBeNull();
    expect(boot.error?.code).toBe("daemon_incompatible");
    expect(boot.error?.message).toContain("no CORS support");
  });

  it("uses an explicit query address and removes it from the address bar", () => {
    window.history.replaceState(
      null,
      "",
      "/app?daemon=http%3A%2F%2Fweb.test%3A9000",
    );
    const boot = createBridge(
      {
        mock: "0",
        scenario: undefined,
        daemonUrl: "http://env.test",
      },
      {
        ...LOCATION,
        search: window.location.search,
      },
    );
    expect(boot.baseUrl).toBe("http://web.test:9000");
    expect(window.location.search).toBe("");
    expect(window.sessionStorage.getItem("spirit.daemon.base")).toBe(
      "http://web.test:9000",
    );
    boot.bridge?.dispose?.();
  });

  it("builds the setup redirect with the daemon query parameter", () => {
    expect(setupRedirectUrl("http://web.test:9000/", LOCATION)).toBe(
      "/app?daemon=http%3A%2F%2Fweb.test%3A9000",
    );
  });
});
