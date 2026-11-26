/**
 * WebCrypto implementation of the relay's end-to-end channel (§6.5).
 *
 * - ephemeral P-256 ECDH;
 * - HKDF-SHA256 with the exact spec info strings;
 * - AES-256-GCM with the 4+8 byte nonce and the 32-byte header as AAD;
 * - Ed25519 verification of the machine's handshake signature.
 *
 * The relay never sees a private key or plaintext. If a browser lacks a required
 * WebCrypto primitive the client fails closed with `cloud_crypto_unsupported`;
 * there is no plaintext fallback.
 */

import {
  cloudError,
  type CloudError,
  type RelayPublicJwk,
} from "@stackspirit/ui";

import {
  RELAY_MAX_DATA_BYTES,
  RELAY_SUITE,
  REMOTE_MAX_PLAINTEXT_BYTES,
  bytesToBase64Url,
  hexToBytes,
  relayTranscript,
  ulidToBytes,
  utf8Bytes,
} from "./relayProtocol";

export interface RelayHandshakeInput {
  channelId: string;
  machineKey: RelayPublicJwk;
  machineNonce: string;
  machineKeyId: string;
  machineSignature: string;
  machineId: string;
  accountId: string;
  ticketJti: string;
  /** The machine's long-lived Ed25519 public key from this client's own ticket. */
  machinePublicKeyHex: string;
  /** The `mkey_id` claim, checked against the accept frame. */
  expectedMachineKeyId: string;
}

export interface RelaySecureChannel {
  /** Encrypts one JSON remote frame into a binary data frame (with sequence). */
  sealFrame(plaintext: string): Promise<ArrayBuffer>;
  /** Validates and decrypts one binary data frame. */
  openFrame(frame: ArrayBuffer): Promise<string>;
}

export interface RelayHandshake {
  readonly clientKey: RelayPublicJwk;
  readonly clientNonce: string;
  complete(input: RelayHandshakeInput): Promise<RelaySecureChannel>;
}

export interface RelayCryptoProvider {
  createHandshake(): Promise<RelayHandshake>;
}

interface CryptoApi {
  subtle: SubtleCrypto;
  getRandomValues<T extends ArrayBufferView>(array: T): T;
}

export interface WebCryptoProviderOptions {
  /** Injectable for tests (Node's `webcrypto`). Defaults to `globalThis.crypto`. */
  crypto?: CryptoApi;
}

function cryptoApi(options: WebCryptoProviderOptions): CryptoApi {
  const api = options.crypto ?? (globalThis.crypto as CryptoApi | undefined);
  if (!api || !api.subtle) {
    throw cloudError(
      "cloud_crypto_unsupported",
      "This browser does not expose WebCrypto, so the encrypted relay cannot start.",
      "Use a current browser over HTTPS. Local server mode needs no relay.",
    );
  }
  return api;
}

function cryptoFailure(message: string): CloudError {
  return cloudError(
    "relay_crypto",
    message,
    "The relay channel was closed. No plaintext fallback exists.",
  );
}

function concatBytes(...parts: Uint8Array<ArrayBuffer>[]): Uint8Array<ArrayBuffer> {
  const total = parts.reduce((sum, part) => sum + part.length, 0);
  const output = new Uint8Array(total);
  let offset = 0;
  for (const part of parts) {
    output.set(part, offset);
    offset += part.length;
  }
  return output;
}

function nonce32(api: CryptoApi): string {
  const bytes = new Uint8Array(32);
  api.getRandomValues(bytes);
  return bytesToBase64Url(bytes);
}

async function exportPublicJwk(api: CryptoApi, key: CryptoKey): Promise<RelayPublicJwk> {
  const jwk = await api.subtle.exportKey("jwk", key);
  if (typeof jwk.x !== "string" || typeof jwk.y !== "string") {
    throw cloudError("cloud_crypto_unsupported", "WebCrypto did not export a P-256 public key.");
  }
  return { kty: "EC", crv: "P-256", x: jwk.x, y: jwk.y };
}

async function importPeerKey(
  api: CryptoApi,
  jwk: RelayPublicJwk,
): Promise<CryptoKey> {
  try {
    return await api.subtle.importKey(
      "jwk",
      { kty: "EC", crv: "P-256", x: jwk.x, y: jwk.y, ext: true },
      { name: "ECDH", namedCurve: "P-256" },
      false,
      [],
    );
  } catch {
    throw cryptoFailure("The machine's ephemeral P-256 key is not a valid curve point.");
  }
}

async function verifyEd25519(
  api: CryptoApi,
  publicKeyHex: string,
  signatureHex: string,
  transcript: string,
): Promise<void> {
  let key: CryptoKey;
  try {
    key = await api.subtle.importKey(
      "raw",
      hexToBytes(publicKeyHex),
      { name: "Ed25519" },
      false,
      ["verify"],
    );
  } catch {
    throw cloudError(
      "cloud_crypto_unsupported",
      "This browser cannot verify Ed25519 machine signatures.",
      "Use a current browser with WebCrypto Ed25519 support.",
    );
  }
  let valid = false;
  try {
    valid = await api.subtle.verify(
      { name: "Ed25519" },
      key,
      hexToBytes(signatureHex),
      utf8Bytes(transcript),
    );
  } catch {
    throw cryptoFailure("The machine handshake signature could not be verified.");
  }
  if (!valid) {
    throw cryptoFailure("The machine handshake signature did not match its published key.");
  }
}

export interface DeriveChannelInput {
  channelId: string;
  localPrivateKey: CryptoKey;
  peerPublicJwk: RelayPublicJwk;
  transcript: string;
  direction: "client" | "machine";
  crypto?: CryptoApi;
}

/**
 * Derives both directional keys. Exported so the protocol test can build the
 * machine side of the same handshake and prove a real ciphertext round trip.
 */
export async function deriveSecureChannel(input: DeriveChannelInput): Promise<RelaySecureChannel> {
  const api = cryptoApi({ crypto: input.crypto });
  const peerKey = await importPeerKey(api, input.peerPublicJwk);
  let shared: ArrayBuffer;
  try {
    shared = await api.subtle.deriveBits(
      { name: "ECDH", public: peerKey },
      input.localPrivateKey,
      256,
    );
  } catch {
    throw cryptoFailure("The P-256 ECDH agreement failed.");
  }
  const sharedBytes = new Uint8Array(shared);
  if (sharedBytes.every((byte) => byte === 0)) {
    throw cryptoFailure("The P-256 ECDH result was the all-zero point.");
  }
  const salt = new Uint8Array(await api.subtle.digest("SHA-256", utf8Bytes(input.transcript)));
  const ikm = await api.subtle.importKey("raw", sharedBytes, "HKDF", false, ["deriveBits"]);
  const expand = async (info: Uint8Array<ArrayBuffer>, bits: number): Promise<Uint8Array<ArrayBuffer>> =>
    new Uint8Array(
      await api.subtle.deriveBits(
        { name: "HKDF", hash: "SHA-256", salt, info },
        ikm,
        bits,
      ),
    );
  const c2mInfo = concatBytes(utf8Bytes("stackspirit c2m key"), Uint8Array.of(1));
  const m2cInfo = concatBytes(utf8Bytes("stackspirit m2c key"), Uint8Array.of(1));
  const c2mIvInfo = utf8Bytes("stackspirit c2m iv");
  const m2cIvInfo = utf8Bytes("stackspirit m2c iv");
  const [c2m, m2c, c2mIv, m2cIv] = await Promise.all([
    expand(c2mInfo, 256),
    expand(m2cInfo, 256),
    expand(c2mIvInfo, 32),
    expand(m2cIvInfo, 32),
  ]);
  const sealKeyBytes = input.direction === "client" ? c2m : m2c;
  const openKeyBytes = input.direction === "client" ? m2c : c2m;
  const sealIv = input.direction === "client" ? c2mIv : m2cIv;
  const openIv = input.direction === "client" ? m2cIv : c2mIv;
  const [sealKey, openKey] = await Promise.all([
    api.subtle.importKey("raw", sealKeyBytes, { name: "AES-GCM" }, false, ["encrypt"]),
    api.subtle.importKey("raw", openKeyBytes, { name: "AES-GCM" }, false, ["decrypt"]),
  ]);
  return createChannel(api, {
    channelId: input.channelId,
    sealKey,
    openKey,
    sealIv,
    openIv,
  });
}

function createChannel(
  api: CryptoApi,
  input: {
    channelId: string;
    sealKey: CryptoKey;
    openKey: CryptoKey;
    sealIv: Uint8Array<ArrayBuffer>;
    openIv: Uint8Array<ArrayBuffer>;
  },
): RelaySecureChannel {
  const channelId = ulidToBytes(input.channelId);
  let outgoing = 0;
  let incoming = 0;

  function header(sequence: number): Uint8Array<ArrayBuffer> {
    const bytes = new Uint8Array(32);
    bytes.set([0x53, 0x53, 0x52, 0x31], 0); // "SSR1"
    bytes[4] = 0x01; // version
    bytes[5] = 0x01; // data
    // flags 0x0000
    bytes.set(channelId, 8);
    new DataView(bytes.buffer).setBigUint64(24, BigInt(sequence), false);
    return bytes;
  }

  function iv(base: Uint8Array<ArrayBuffer>, sequence: number): Uint8Array<ArrayBuffer> {
    const bytes = new Uint8Array(12);
    bytes.set(base, 0);
    new DataView(bytes.buffer).setBigUint64(4, BigInt(sequence), false);
    return bytes;
  }

  return {
    async sealFrame(plaintext: string): Promise<ArrayBuffer> {
      const encoded = utf8Bytes(plaintext);
      if (encoded.byteLength > REMOTE_MAX_PLAINTEXT_BYTES) {
        throw cloudError(
          "relay_protocol",
          "A remote frame exceeds the 768 KiB plaintext limit.",
        );
      }
      outgoing += 1;
      const head = header(outgoing);
      let ciphertext: ArrayBuffer;
      try {
        ciphertext = await api.subtle.encrypt(
          {
            name: "AES-GCM",
            iv: iv(input.sealIv, outgoing),
            additionalData: head,
            tagLength: 128,
          },
          input.sealKey,
          encoded,
        );
      } catch {
        throw cryptoFailure("The relay frame could not be encrypted.");
      }
      const frame = new Uint8Array(36 + ciphertext.byteLength);
      frame.set(head, 0);
      new DataView(frame.buffer).setUint32(32, ciphertext.byteLength, false);
      frame.set(new Uint8Array(ciphertext), 36);
      if (frame.byteLength > RELAY_MAX_DATA_BYTES) {
        throw cloudError(
          "relay_frame_too_large",
          "The encrypted relay frame exceeds the 1 MiB cap.",
        );
      }
      return frame.buffer;
    },

    async openFrame(frame: ArrayBuffer): Promise<string> {
      const bytes = new Uint8Array(frame);
      if (bytes.byteLength > RELAY_MAX_DATA_BYTES) {
        throw cloudError(
          "relay_frame_too_large",
          "The relay frame exceeds the 1 MiB cap.",
        );
      }
      if (bytes.byteLength < 36) throw cryptoFailure("The relay data frame is truncated.");
      const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      if (bytes[0] !== 0x53 || bytes[1] !== 0x53 || bytes[2] !== 0x52 || bytes[3] !== 0x31) {
        throw cryptoFailure("The relay data frame has a bad magic value.");
      }
      if (bytes[4] !== 0x01) throw cryptoFailure("The relay data frame uses an unsupported version.");
      if (bytes[5] !== 0x01) throw cryptoFailure("The relay data frame is not a data frame.");
      if (bytes[6] !== 0 || bytes[7] !== 0) throw cryptoFailure("The relay data frame has unknown flags.");
      for (let index = 0; index < 16; index += 1) {
        if (bytes[8 + index] !== channelId[index]) {
          throw cryptoFailure("The relay data frame belongs to another channel.");
        }
      }
      const sequence = Number(view.getBigUint64(24, false));
      if (!Number.isSafeInteger(sequence) || sequence !== incoming + 1) {
        throw cryptoFailure("The relay data frame sequence is not the next expected value.");
      }
      const length = view.getUint32(32, false);
      if (length < 16 || length !== bytes.byteLength - 36) {
        throw cryptoFailure("The relay data frame ciphertext length is wrong.");
      }
      let plaintext: ArrayBuffer;
      try {
        plaintext = await api.subtle.decrypt(
          {
            name: "AES-GCM",
            iv: iv(input.openIv, sequence),
            additionalData: bytes.subarray(0, 32),
            tagLength: 128,
          },
          input.openKey,
          bytes.subarray(36),
        );
      } catch {
        throw cryptoFailure("The relay data frame failed authentication.");
      }
      incoming = sequence;
      if (plaintext.byteLength > REMOTE_MAX_PLAINTEXT_BYTES) {
        throw cloudError("relay_protocol", "A remote frame exceeds the 768 KiB plaintext limit.");
      }
      try {
        return new TextDecoder().decode(plaintext);
      } catch {
        throw cryptoFailure("The relay data frame is not valid UTF-8.");
      }
    },
  };
}

/** The production WebCrypto provider. Constructing it makes no request and no key. */
export function createWebCryptoProvider(options: WebCryptoProviderOptions = {}): RelayCryptoProvider {
  return {
    async createHandshake(): Promise<RelayHandshake> {
      const api = cryptoApi(options);
      let pair: CryptoKeyPair;
      try {
        pair = (await api.subtle.generateKey(
          { name: "ECDH", namedCurve: "P-256" },
          true,
          ["deriveBits"],
        )) as CryptoKeyPair;
      } catch {
        throw cloudError(
          "cloud_crypto_unsupported",
          "This browser cannot generate a P-256 ECDH key.",
          "Use a current browser over HTTPS.",
        );
      }
      const clientKey = await exportPublicJwk(api, pair.publicKey);
      const clientNonce = nonce32(api);
      return {
        clientKey,
        clientNonce,
        async complete(input: RelayHandshakeInput): Promise<RelaySecureChannel> {
          if (input.machineKeyId !== input.expectedMachineKeyId) {
            throw cryptoFailure("The machine presented a different relay key than its ticket.");
          }
          const transcript = relayTranscript({
            channelId: input.channelId,
            clientKey,
            machineKey: input.machineKey,
            clientNonce,
            machineNonce: input.machineNonce,
            machineId: input.machineId,
            accountId: input.accountId,
            ticketJti: input.ticketJti,
          });
          if (input.machineSignature.length === 0) {
            throw cryptoFailure("The machine did not sign the handshake transcript.");
          }
          await verifyEd25519(api, input.machinePublicKeyHex, input.machineSignature, transcript);
          return deriveSecureChannel({
            channelId: input.channelId,
            localPrivateKey: pair.privateKey,
            peerPublicJwk: input.machineKey,
            transcript,
            direction: "client",
            crypto: api,
          });
        },
      };
    },
  };
}

/** Exposed for diagnostics/tests; not part of the wire contract. */
export { RELAY_SUITE };
