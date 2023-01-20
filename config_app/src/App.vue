<script setup lang="ts">
// This starter template is using Vue 3 <script setup> SFCs
// Check out https://vuejs.org/api/sfc-script-setup.html#script-setup
import Greet from "./components/Greet.vue";
import Graph3D from "./components/map_editor/Graph3D.vue"
import { invoke } from "@tauri-apps/api/tauri";


async function list_devices() {
  // Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
  let x = await invoke("list_devices", {});
  console.log(x);
}

</script>

<template>
  <div class="container">
    <h1>Welcome to Tauri!</h1>

    <Greet />
  </div>
  <button type="button" @click="list_devices()">List devices</button>
  <Graph3D></Graph3D>
</template>

<style scoped>
.logo.vite:hover {
  filter: drop-shadow(0 0 2em #747bff);
}

.logo.vue:hover {
  filter: drop-shadow(0 0 2em #249b73);
}
</style>


