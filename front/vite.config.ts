import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

// https://vite.dev/config/
export default defineConfig({
  base: process.env.VITE_BASE,
  plugins: [react({ compiler: true }), tailwindcss()],
  resolve: {
    tsconfigPaths: true,
  },
})
