export type CyberAccessProgram = "standard" | "daybreak_blue" | "daybreak_red";

export type TurnOptions = {
  /** Experimental cyber program for this turn. Omission preserves automatic behavior. */
  cyberAccessProgram?: CyberAccessProgram;
  /** JSON schema describing the expected agent output. */
  outputSchema?: unknown;
  /** AbortSignal to cancel the turn. */
  signal?: AbortSignal;
};
