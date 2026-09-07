import type { ResourceContext } from "./schema";

/** Prefer the observed name; opaque identity remains available in resource details. */
export function resourceName(id: string, context?: ResourceContext | null): string {
  return context?.name || id.split("/").filter(Boolean).at(-1) || id;
}
export function locationText(context?: ResourceContext | null): string {
  if (!context) return "Location not recorded";
  return [context.scope, context.zone ?? context.region, context.namespace].filter(Boolean).join(" · ");
}
export function resourceType(context?: ResourceContext | null): string {
  return context ? `${context.provider.toUpperCase()} · ${context.service.replaceAll("-", " ")}` : "Type not recorded";
}
