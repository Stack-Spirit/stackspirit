import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

// npm runs the workspace script from apps/spirit-web. Fall back to the monorepo
// root form as well so the check also works when Vitest is launched from the top.
const webRoot = existsSync(resolve(process.cwd(), "index.html"))
  ? process.cwd()
  : resolve(process.cwd(), "apps/spirit-web");
const publicFile = (path: string) => resolve(webRoot, "public", path);
const indexFile = resolve(webRoot, "index.html");

describe("Web favicon", () => {
  it("ships the crisp StackSpirit frog icons in public/", () => {
    const ico = readFileSync(publicFile("favicon.ico"));
    expect(ico.length).toBeGreaterThan(0);
    // ICO header: reserved 0, type 1, at least one image.
    expect([...ico.subarray(0, 4)]).toEqual([0, 0, 1, 0]);

    const svg = readFileSync(publicFile("icon-32.svg"), "utf8");
    expect(svg.startsWith("<svg")).toBe(true);

    for (const name of ["icon-32.png", "icon-16.png"]) {
      const png = readFileSync(publicFile(name));
      expect([...png.subarray(0, 8)]).toEqual([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    }
  });

  it("declares the SVG, 32/16 px PNG and ICO favicons in index.html", () => {
    const html = readFileSync(indexFile, "utf8");
    expect(html).toContain('<link rel="icon" href="/icon-32.svg" type="image/svg+xml" />');
    expect(html).toContain('<link rel="icon" href="/icon-32.png" sizes="32x32" type="image/png" />');
    expect(html).toContain('<link rel="icon" href="/icon-16.png" sizes="16x16" type="image/png" />');
    expect(html).toContain('<link rel="icon" href="/favicon.ico" sizes="any" />');
  });
});
