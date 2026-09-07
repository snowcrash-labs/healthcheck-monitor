import { render } from "@solidjs/web";
import { createRouter } from "@solidjs/router";
import { lazy } from "solid-js";
import { Root } from "./layout";
import Overview from "./overview";
import "./styles.css";
import "./investigation.css";
import "./costs.css";

const Checks = lazy(() => import("./checks"));
const CheckDetail = lazy(() => import("./check-detail"));
const Target = lazy(() => import("./target"));
const Findings = lazy(() => import("./findings"));
const Resources = lazy(() => import("./resources"));
const Detail = lazy(() => import("./detail"));
const History = lazy(() => import("./history"));
const MonitorStatus = lazy(() => import("./monitor-status"));
const Costs = lazy(() => import("./costs"));
const RecentErrors = lazy(() => import("./recent-errors"));
const Router = createRouter({ routes: [
  { path: "/", component: Overview },
  { path: "/targets/:target", component: Target },
  { path: "/checks", component: Checks },
  { path: "/checks/:target/:check", component: CheckDetail },
  { path: "/findings", component: Findings },
  { path: "/problems", component: Findings },
  { path: "/monitor", component: MonitorStatus },
  { path: "/costs", component: Costs },
  { path: "/recent-errors", component: RecentErrors },
  { path: "/resources", component: Resources },
  { path: "/resources/:id", component: Detail },
  { path: "/history", component: History },
  { path: "*", component: () => <div class="empty"><h1>Page not found</h1><a href="/">Return to overview</a></div> },
] });
const host = document.getElementById("app");
if (host) render(() => <Router>{(props) => <Root>{props.children}</Root>}</Router>, host);
