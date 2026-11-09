/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      fontFamily: {
        sans: [
          "Segoe UI Variable Text",
          "Segoe UI",
          "Microsoft YaHei UI",
          "system-ui",
          "sans-serif",
        ],
      },
    },
  },
  plugins: [],
};
