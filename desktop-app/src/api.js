import { invoke } from "@tauri-apps/api/core";

export const api = {
  invokeCommand(command, args) {
    return invoke(command, args);
  }
};

