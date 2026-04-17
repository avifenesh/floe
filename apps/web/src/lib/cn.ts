import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/** Merge class names, dedupe conflicts the shadcn way. */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
