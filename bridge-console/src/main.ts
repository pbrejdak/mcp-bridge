import { mount } from "svelte";
import App from "./App.svelte";

const root = document.getElementById("app");
if (!root) {
  throw new Error("#app mount point not found in index.html");
}

const app = mount(App, { target: root });

export default app;
