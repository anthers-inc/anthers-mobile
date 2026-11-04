// SPDX-License-Identifier: Apache-2.0
//
// The version script's suite. The bump decision, the calver arithmetic and the web pin are
// the places a quiet mistake publishes a wrong number, so they are pinned here rather than
// trusted to a green release build: a release build succeeds against whatever version each
// tool reads, which is exactly how four fields drift apart unnoticed.
//
// ⚠️ Importing this module must never bump anything — `main` sits behind `import.meta.main`,
// because a test import that executes the flow would silently write a version into the tree.

import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
	FIELDS,
	WEB_PIN,
	agreedVersion,
	bumpPlan,
	nextVersion,
	readField,
	webPin,
	writeField,
} from "./promote-version.ts";

const ROOT = join(import.meta.dir, "..");
const NOV = new Date(2026, 10, 3);

describe("nextVersion", () => {
	it("counts up inside a month", () => {
		expect(nextVersion("2026.11.0", NOV)).toBe("2026.11.1");
		expect(nextVersion("2026.11.37", NOV)).toBe("2026.11.38");
	});

	it("starts a new month at .0", () => {
		expect(nextVersion("2026.10.4", NOV)).toBe("2026.11.0");
		expect(nextVersion("2025.12.9", NOV)).toBe("2026.11.0");
	});

	it("refuses anything that is not calver", () => {
		expect(() => nextVersion("0.1.0", NOV)).toThrow(/not calver/);
		expect(() => nextVersion("v2026.11.0", NOV)).toThrow(/not calver/);
	});
});

describe("bumpPlan", () => {
	it("writes on a clean tree", () => {
		expect(bumpPlan("2026.11.0", "2026.11.0", undefined, NOV)).toEqual({
			action: "write",
			target: "2026.11.1",
		});
	});

	it("converges when a resumed promote already wrote the bump", () => {
		expect(bumpPlan("2026.11.0", "2026.11.1", undefined, NOV)).toEqual({
			action: "no-op",
			target: "2026.11.1",
		});
	});

	it("refuses a value no step wrote", () => {
		expect(bumpPlan("2026.11.0", "2026.11.7", undefined, NOV)).toEqual({
			action: "refuse",
			target: "2026.11.1",
			committed: "2026.11.0",
			working: "2026.11.7",
		});
	});

	it("lets a pin overwrite a stray value", () => {
		expect(bumpPlan("2026.11.0", "2026.11.7", "2026.11.3", NOV)).toEqual({
			action: "write",
			target: "2026.11.3",
		});
		expect(bumpPlan("2026.11.0", "2026.11.3", "2026.11.3", NOV)).toEqual({
			action: "no-op",
			target: "2026.11.3",
		});
	});
});

describe("the four version fields", () => {
	for (const { path, pattern } of FIELDS) {
		it(`reads and rewrites ${path}`, () => {
			const source = readFileSync(join(ROOT, path), "utf8");
			const version = readField(source, pattern);
			expect(version).not.toBeNull();

			const rewritten = writeField(source, pattern, "2031.2.7");
			expect(readField(rewritten, pattern)).toBe("2031.2.7");
			// Only the version moves: the rest of the file is byte-identical.
			expect(rewritten.replace("2031.2.7", version as string)).toBe(source);
		});
	}

	// The checkout's own invariant, and the one `--check` guards in CI: four fields,
	// one calver version. A tree mid-bump is the only legitimate way this fails.
	it("agrees across all four in this checkout", () => {
		const read = (path: string) => readFileSync(join(ROOT, path), "utf8");
		expect(agreedVersion(read, "test")).toMatch(/^\d{4}\.\d{1,2}\.\d+$/);
	});
});

describe("the web pin", () => {
	it("reads and rewrites package.json's anthersWeb", () => {
		const source = readFileSync(join(ROOT, WEB_PIN.path), "utf8");
		const pin = readField(source, WEB_PIN.pattern);
		expect(pin).toMatch(/^v\d{4}\.\d{1,2}\.\d+$/);

		const rewritten = writeField(source, WEB_PIN.pattern, "v2031.2.7");
		expect(readField(rewritten, WEB_PIN.pattern)).toBe("v2031.2.7");
		expect(rewritten.replace("v2031.2.7", pin as string)).toBe(source);
	});

	it("carries a release tag in this checkout", () => {
		const read = (path: string) => readFileSync(join(ROOT, path), "utf8");
		expect(webPin(read, "test")).toMatch(/^v\d{4}\.\d{1,2}\.\d+$/);
	});
});
