export type MachineState = "idle" | "running" | "failed" | "done";

export function step(state: MachineState): MachineState {
  if (state === "idle") return "running";
  if (state === "running") return "done";
  if (state === "failed") return "idle";
  return state;
}
