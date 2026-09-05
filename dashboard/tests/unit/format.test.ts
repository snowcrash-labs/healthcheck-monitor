import { describe, expect, it } from "vitest";
import { age, stale, utc } from "../../src/format";
import { query, resourceId, resourcePath } from "../../src/api";
import { overview } from "../../src/schema";

describe("observation age", () => {
  it("keeps missing and stale evidence distinct from a current update", () => {
    expect(age(null, 1000)).toBe("Awaiting observation");
    expect(stale("2026-09-05T00:00:00Z", Date.parse("2026-09-05T01:00:00Z"))).toBe(true);
    expect(age("2026-09-05T00:00:00Z", Date.parse("2026-09-05T01:00:00Z"))).toBe("1h ago");
  });
  it("formats UTC independently of the browser timezone", () => expect(utc("2026-09-05T03:00:00+03:00")).toBe("2026-09-05 00:00:00 UTC"));
});
describe("API boundaries", () => {
  it("round-trips slashes, percent escapes, and Unicode in resource routes", () => {
    for (const id of ["dev/pods/main", "dev/name%2Fpart?x&y", "dev/노드/one"]) expect(resourceId(resourcePath(id).split("/").at(-1))).toBe(id);
    expect(resourceId("%broken")).toBeUndefined();
  });
  it("encodes resource identifiers without altering their scope", () => expect(query("/api/v1/resource", { id: "dev/pod/a?b&c", empty: undefined })).toBe("/api/v1/resource?id=dev%2Fpod%2Fa%3Fb%26c"));
  it("rejects an incomplete overview instead of turning absent counts into zero", () => expect(overview.safeParse({ totals: { error_findings: 0 } }).success).toBe(false));
});
