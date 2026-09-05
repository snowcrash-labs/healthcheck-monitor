import { render } from "@solidjs/web";
import { createRouter } from "@solidjs/router";
import { lazy } from "solid-js";
import { Root } from "./layout";
import Overview from "./overview";
import "./styles.css";

const Findings = lazy(() => import("./findings"));
const Resources = lazy(() => import("./resources"));
const Detail = lazy(() => import("./detail"));
const History = lazy(() => import("./history"));
const Router = createRouter({ routes: [
  { path: "/", component: Overview },
  { path: "/findings", component: Findings },
  { path: "/resources", component: Resources },
  { path: "/resources/:id", component: Detail },
  { path: "/history", component: History },
  { path: "*", component: () => <div class="empty"><h1>Page not found</h1><a href="/">Return to overview</a></div> },
] });
const host = document.getElementById("app");
if (host) render(() => <Router>{(props) => <Root>{props.children}</Root>}</Router>, host);
