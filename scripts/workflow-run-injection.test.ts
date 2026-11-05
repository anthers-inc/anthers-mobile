// SPDX-License-Identifier: Apache-2.0
//
// 🚨 **A `${{ }}` expression inside a `run:` script is substituted before the shell parses
// the line, so its value is shell source.** A keystore password carrying a quote, a `$(…)`
// or a newline runs as syntax — and the keystore is the app's update identity, so a command
// that runs there runs with the key. This is the shape the release workflow shipped until
// 2026-11-05. The fix is always the same: put the value in `env:` on the step and read
// `"$NAME"` inside the script, where every character is inert.
//
// The rule covers more than secrets on purpose. A version read out of `package.json` or a
// ref name is the same injection shape with a quieter value, and `env:` costs one line.

import { expect, test } from "bun:test";
import { readdir } from "node:fs/promises";
import { join } from "node:path";

const WORKFLOWS = join(import.meta.dir, "..", ".github", "workflows");

interface Step {
	name?: string;
	run?: unknown;
}

interface Job {
	steps?: Step[];
}

interface Workflow {
	jobs?: Record<string, Job>;
}

test("no workflow interpolates an expression into a run script", async () => {
	const names = (await readdir(WORKFLOWS)).filter((name) => /\.ya?ml$/.test(name));

	// An empty directory reads as a clean pass, which is the vacuous green this guard
	// exists to avoid one level up.
	expect(names.length, `no workflow files found in ${WORKFLOWS}`).toBeGreaterThan(0);

	const offenders: string[] = [];
	for (const name of names) {
		const text = await Bun.file(join(WORKFLOWS, name)).text();
		const workflow = Bun.YAML.parse(text) as Workflow;

		for (const [jobId, job] of Object.entries(workflow.jobs ?? {})) {
			for (const [index, step] of (job.steps ?? []).entries()) {
				if (typeof step.run !== "string") continue;
				if (!/\$\{\{/.test(step.run)) continue;
				offenders.push(`${name}: jobs.${jobId}.steps[${index}] (${step.name ?? "unnamed"})`);
			}
		}
	}

	expect(
		offenders,
		"An expression reached a run script. Pass it through `env:` on the step and read the " +
			"variable inside the script — see the Configure signing step in release.yml.",
	).toEqual([]);
});
