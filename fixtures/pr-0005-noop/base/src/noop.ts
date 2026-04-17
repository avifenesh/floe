export type Status = "on" | "off";

export function toggle(s: Status): Status {
  return s === "on" ? "off" : "on";
}
