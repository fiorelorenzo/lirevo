import { goto } from "$app/navigation";
import type { Route } from "./tauri";

const ROUTE_PATHS: Record<Route, string> = {
  home: "/",
  settings: "/settings",
  wizard: "/wizard",
};

export function navigate(route: Route): void {
  void goto(ROUTE_PATHS[route]);
}
