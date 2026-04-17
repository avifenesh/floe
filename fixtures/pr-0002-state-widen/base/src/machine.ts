export type MachineState = "idle" | "running" | "done";

export function step(state: MachineState): MachineState {
  if (state === "idle") return "running";
  if (state === "running") return "done";
  return state;
}
