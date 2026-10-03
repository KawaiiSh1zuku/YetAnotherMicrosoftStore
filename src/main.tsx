import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

const testClient = import.meta.env.DEV ? window.__YAMS_TEST_CLIENT__ : undefined;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App client={testClient} />
  </React.StrictMode>,
);
