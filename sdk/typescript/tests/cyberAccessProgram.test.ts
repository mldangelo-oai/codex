import { describe, expect, it } from "@jest/globals";

import type { CyberAccessProgram, ThreadEvent } from "../src/index";
import {
  assistantMessage,
  responseCompleted,
  responseStarted,
  sse,
  startResponsesTestProxy,
} from "./responsesProxy";
import { createTestClient } from "./testCodex";

describe("cyber access programs", () => {
  const cases: {
    method: "run" | "runStreamed";
    resume: boolean;
    programs: (CyberAccessProgram | undefined)[];
  }[] = [
    { method: "run", resume: false, programs: ["daybreak_blue", "daybreak_red", undefined] },
    { method: "runStreamed", resume: true, programs: [undefined, "standard", undefined] },
  ];

  it.each(cases)(
    "applies each $method turn's selection with API-key auth (resume: $resume)",
    async ({ method, resume, programs }) => {
      const { url, close, requests } = await startResponsesTestProxy({
        responseBodies: programs.map((_, index) =>
          sse(
            responseStarted(`response_${index}`),
            assistantMessage(`Reply ${index}`, `item_${index}`),
            responseCompleted(`response_${index}`),
          ),
        ),
      });
      const { client, cleanup } = createTestClient({
        baseUrl: url,
        apiKey: "test-key",
        // Cyber programs are only forwarded by the built-in OpenAI provider.
        config: { model_provider: "openai" },
        inheritEnv: false,
        env: { CODEX_HOME: process.env.CODEX_HOME!, PATH: process.env.PATH ?? "" },
      });

      try {
        let thread = client.startThread({ skipGitRepoCheck: true });
        for (const [index, cyberAccessProgram] of programs.entries()) {
          if (resume && index > 0) {
            expect(thread.id).toEqual(expect.any(String));
            thread = client.resumeThread(thread.id!, { skipGitRepoCheck: true });
          }
          const prompt = `Turn ${index}`;
          if (method === "run") {
            const turn = await thread.run(prompt, { cyberAccessProgram });
            expect(turn.finalResponse).toBe(`Reply ${index}`);
          } else {
            const turn = await thread.runStreamed(prompt, { cyberAccessProgram });
            const events: ThreadEvent[] = [];
            for await (const event of turn.events) {
              events.push(event);
            }
            expect(events.at(-1)?.type).toBe("turn.completed");
          }
        }

        expect(requests.map((request) => request.json.access_programs)).toEqual(
          programs.map((program) => (program === undefined ? undefined : { cyber: program })),
        );
        expect(requests.map((request) => request.headers.authorization)).toEqual(
          programs.map(() => "Bearer test-key"),
        );
      } finally {
        cleanup();
        await close();
      }
    },
    // Each case completes three CLI process lifecycles.
    30_000,
  );
});
