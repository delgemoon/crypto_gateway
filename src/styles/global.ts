import { createGlobalStyle } from "styled-components"

export default createGlobalStyle`
  *, *::before, *::after {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }

  html, body, #root {
    height: 100%;
    width: 100%;
    overflow: hidden;
  }

  body {
    background-color: #0d1117;
    color: #e8edf4;
    font-family: 'Inter', 'Segoe UI', system-ui, -apple-system, sans-serif;
    font-size: 14px;
    line-height: 1.5;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
  }

  #root {
    display: flex;
    flex-direction: column;
  }

  /* Reset browser defaults that break styled-components */
  input, button, select, textarea {
    font-family: inherit;
    font-size: inherit;
  }

  button {
    cursor: pointer;
  }

  ::-webkit-scrollbar {
    width: 6px;
    height: 6px;
  }
  ::-webkit-scrollbar-track { background: #0d1117; }
  ::-webkit-scrollbar-thumb { background: #2a3a52; border-radius: 3px; }
  ::-webkit-scrollbar-thumb:hover { background: #3a5070; }
`
