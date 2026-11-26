import { webcrypto } from "node:crypto";

import { describe, expect, it } from "vitest";

import {
  canonicalJwk,
  bytesToHex,
  relayTranscript,
  ulidToBytes,
} from "./relayProtocol";
import {
  createWebCryptoProvider,
  deriveSecureChannel,
  type RelayCryptoProvider,
} from "./relayCrypto";

const CHANNEL_ID = "01JQ8Z6K4W5N7R9T2V3X4Y5Z6A";
const MACHINE_ID = "mch_01JQ8Z6K4W5N7R9T2V3X4Y5Z6A";
const ACCOUNT_ID = "acct_01JQ8Z6K4W5N7R9T2V3X4Y5Z6A";
const JTI = "jti_01JQ8Z6K4W5N7R9T2V3X4Y5Z6A";

const cryptoApi = webcrypto as unknown as {
  subtle: SubtleCrypto;
  getRandomValues<T extends ArrayBufferView>(array: T): T;
};

interface MachineHandshake {
  machineJwk: { kty: "EC"; crv: "P-256"; x: string; y: string };
  publicKeyHex: string;
  signatureHex: string;
  privateKey: CryptoKey;
  transcript: string;
}

async function machineSide(
  clientKey: { kty: "EC"; crv: "P-256"; x: string; y: string },
  clientNonce: string,
): Promise<MachineHandshake> {
  const ecdh = (await cryptoApi.subtle.generateKey(
    { name: "ECDH", namedCurve: "P-256" },
    true,
    ["deriveBits"],
  )) as CryptoKeyPair;
  const jwk = await cryptoApi.subtle.exportKey("jwk", ecdh.publicKey);
  const machineJwk = {
    kty: "EC" as const,
    crv: "P-256" as const,
    x: jwk.x ?? "",
    y: jwk.y ?? "",
  };
  const machineNonce = "machine-nonce";
  const transcript = relayTranscript({
    channelId: CHANNEL_ID,
    clientKey,
    machineKey: machineJwk,
    clientNonce,
    machineNonce,
    machineId: MACHINE_ID,
    accountId: ACCOUNT_ID,
    ticketJti: JTI,
  });
  const identity = (await cryptoApi.subtle.generateKey(
    { name: "Ed25519" },
    true,
    ["sign", "verify"],
  )) as CryptoKeyPair;
  const publicKey = new Uint8Array(
    await cryptoApi.subtle.exportKey("raw", identity.publicKey),
  );
  const signature = new Uint8Array(
    await cryptoApi.subtle.sign(
      { name: "Ed25519" },
      identity.privateKey,
      new TextEncoder().encode(transcript),
    ),
  );
  return {
    machineJwk,
    publicKeyHex: bytesToHex(publicKey),
    signatureHex: bytesToHex(signature),
    privateKey: ecdh.privateKey,
    transcript,
  };
}

describe("relay WebCrypto handshake", () => {
  it("derives the same directional keys on both sides and round-trips data frames", async () => {
    const provider: RelayCryptoProvider = createWebCryptoProvider({ crypto: cryptoApi });
    const handshake = await provider.createHandshake();
    const machine = await machineSide(handshake.clientKey, handshake.clientNonce);

    const clientChannel = await handshake.complete({
      channelId: CHANNEL_ID,
      machineKey: machine.machineJwk,
      machineNonce: "machine-nonce",
      machineKeyId: `ed25519:${machine.publicKeyHex}`,
      machineSignature: machine.signatureHex,
      machineId: MACHINE_ID,
      accountId: ACCOUNT_ID,
      ticketJti: JTI,
      machinePublicKeyHex: machine.publicKeyHex,
      expectedMachineKeyId: `ed25519:${machine.publicKeyHex}`,
    });
    const machineChannel = await deriveSecureChannel({
      channelId: CHANNEL_ID,
      localPrivateKey: machine.privateKey,
      peerPublicJwk: handshake.clientKey,
      transcript: machine.transcript,
      direction: "machine",
      crypto: cryptoApi,
    });

    const outbound = await clientChannel.sealFrame('{"type":"hello","v":1}');
    expect(await machineChannel.openFrame(outbound)).toBe('{"type":"hello","v":1}');
    const inbound = await machineChannel.sealFrame('{"type":"hello_ack","v":1}');
    expect(await clientChannel.openFrame(inbound)).toBe('{"type":"hello_ack","v":1}');

    const bytes = new Uint8Array(outbound);
    expect(Array.from(bytes.slice(0, 4))).toEqual([0x53, 0x53, 0x52, 0x31]);
    expect(bytes[4]).toBe(1);
    expect(bytes[5]).toBe(1);
    expect(bytes[6]).toBe(0);
    expect(bytes[7]).toBe(0);
    expect(Array.from(bytes.slice(8, 24))).toEqual(Array.from(ulidToBytes(CHANNEL_ID)));
    expect(new DataView(bytes.buffer).getBigUint64(24, false)).toBe(1n);
    expect(new DataView(bytes.buffer).getUint32(32, false)).toBe(bytes.byteLength - 36);
  });

  it("rejects a substituted machine key id and a bad Ed25519 signature", async () => {
    const provider = createWebCryptoProvider({ crypto: cryptoApi });
    const handshake = await provider.createHandshake();
    const machine = await machineSide(handshake.clientKey, handshake.clientNonce);

    await expect(
      handshake.complete({
        channelId: CHANNEL_ID,
        machineKey: machine.machineJwk,
        machineNonce: "machine-nonce",
        machineKeyId: `ed25519:${"00".repeat(32)}`,
        machineSignature: machine.signatureHex,
        machineId: MACHINE_ID,
        accountId: ACCOUNT_ID,
        ticketJti: JTI,
        machinePublicKeyHex: machine.publicKeyHex,
        expectedMachineKeyId: `ed25519:${machine.publicKeyHex}`,
      }),
    ).rejects.toMatchObject({ code: "relay_crypto" });

    await expect(
      handshake.complete({
        channelId: CHANNEL_ID,
        machineKey: machine.machineJwk,
        machineNonce: "machine-nonce",
        machineKeyId: `ed25519:${machine.publicKeyHex}`,
        machineSignature: "00".repeat(64),
        machineId: MACHINE_ID,
        accountId: ACCOUNT_ID,
        ticketJti: JTI,
        machinePublicKeyHex: machine.publicKeyHex,
        expectedMachineKeyId: `ed25519:${machine.publicKeyHex}`,
      }),
    ).rejects.toMatchObject({ code: "relay_crypto" });
  });

  it("rejects a tampered header and a replayed sequence", async () => {
    const provider = createWebCryptoProvider({ crypto: cryptoApi });
    const handshake = await provider.createHandshake();
    const machine = await machineSide(handshake.clientKey, handshake.clientNonce);
    const clientChannel = await handshake.complete({
      channelId: CHANNEL_ID,
      machineKey: machine.machineJwk,
      machineNonce: "machine-nonce",
      machineKeyId: `ed25519:${machine.publicKeyHex}`,
      machineSignature: machine.signatureHex,
      machineId: MACHINE_ID,
      accountId: ACCOUNT_ID,
      ticketJti: JTI,
      machinePublicKeyHex: machine.publicKeyHex,
      expectedMachineKeyId: `ed25519:${machine.publicKeyHex}`,
    });
    const machineChannel = await deriveSecureChannel({
      channelId: CHANNEL_ID,
      localPrivateKey: machine.privateKey,
      peerPublicJwk: handshake.clientKey,
      transcript: machine.transcript,
      direction: "machine",
      crypto: cryptoApi,
    });

    const first = await clientChannel.sealFrame("one");
    expect(await machineChannel.openFrame(first)).toBe("one");
    const second = await clientChannel.sealFrame("two");
    const tampered = new Uint8Array(second.slice(0));
    tampered[20] = (tampered[20] ?? 0) ^ 0xff;
    await expect(machineChannel.openFrame(tampered.buffer as ArrayBuffer)).rejects.toMatchObject({
      code: "relay_crypto",
    });

    // A rejected frame never advances the expected sequence; the genuine frame
    // is still accepted, and its second use is rejected as a replay.
    expect(await machineChannel.openFrame(second)).toBe("two");
    await expect(machineChannel.openFrame(second)).rejects.toMatchObject({
      code: "relay_crypto",
    });
  });

  it("builds the exact RFC 7638 canonical JWK", () => {
    expect(
      canonicalJwk({ kty: "EC", crv: "P-256", x: "abc", y: "def" }),
    ).toBe('{"crv":"P-256","kty":"EC","x":"abc","y":"def"}');
  });
});
