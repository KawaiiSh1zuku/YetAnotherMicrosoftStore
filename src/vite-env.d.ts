/// <reference types="vite/client" />

interface Window {
  __YAMS_TEST_CLIENT__?: import("./lib/tauri").StoreClient;
}
