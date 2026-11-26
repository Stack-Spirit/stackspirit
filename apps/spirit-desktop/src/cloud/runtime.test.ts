import { afterEach, describe, expect, it, vi } from "vitest";

import { resetCloudAccount, useCloudAccount, type CloudFetch } from "@stackspirit/ui";

import { createDesktopCloudRuntime } from "./runtime";

function deviceResponse(): Response {
  return new Response(
    JSON.stringify({
      device_code: "device-one",
      user_code: "ABCD-EFGH",
      verification_uri: "https://stackspirit.io/device",
      verification_uri_complete: "https://stackspirit.io/device?code=ABCD-EFGH",
      expires_in: 600,
      interval: 5,
    }),
    { status: 200, headers: { "content-type": "application/json" } },
  );
}

afterEach(() => {
  resetCloudAccount();
});

describe("desktop cloud runtime", () => {
  it("configures the account client without making a request or leaving local mode", () => {
    const fetchImpl = vi.fn<CloudFetch>(async () => deviceResponse());
    const runtime = createDesktopCloudRuntime({ fetchImpl });

    expect(fetchImpl).not.toHaveBeenCalled();
    const state = useCloudAccount.getState();
    expect(state.phase).toBe("local");
    expect(state.client).toBe(runtime.client);
    expect(state.relay).toBeNull();
    expect(state.machines).toEqual([]);

    runtime.dispose();
    expect(useCloudAccount.getState().client).toBeNull();
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("signs in against the default StackSpirit API from Settings", async () => {
    const fetchImpl = vi.fn<CloudFetch>(async () => deviceResponse());
    const runtime = createDesktopCloudRuntime({ fetchImpl });

    await useCloudAccount.getState().startSignIn();

    expect(fetchImpl).toHaveBeenCalledTimes(1);
    const [url, init] = fetchImpl.mock.calls[0]!;
    expect(String(url)).toBe("https://api.stackspirit.io/v1/auth/device");
    expect(init?.method).toBe("POST");
    expect(useCloudAccount.getState().phase).toBe("device_pending");
    expect(useCloudAccount.getState().device?.user_code).toBe("ABCD-EFGH");
    runtime.dispose();
  });

  it("keeps a configured loopback API base for local development", async () => {
    const fetchImpl = vi.fn<CloudFetch>(async () => deviceResponse());
    const runtime = createDesktopCloudRuntime({
      apiBaseUrl: "http://127.0.0.1:4310",
      fetchImpl,
    });

    await useCloudAccount.getState().startSignIn();

    const [url] = fetchImpl.mock.calls[0]!;
    expect(String(url)).toBe("http://127.0.0.1:4310/v1/auth/device");
    runtime.dispose();
  });

  it("leaves the local default in place when the optional API base is not secure", () => {
    expect(() =>
      createDesktopCloudRuntime({ apiBaseUrl: "http://api.stackspirit.io" }),
    ).toThrow();
    expect(useCloudAccount.getState().client).toBeNull();
    expect(useCloudAccount.getState().phase).toBe("local");
  });
});
