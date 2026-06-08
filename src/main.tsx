import React from 'react';
import ReactDOM from "react-dom/client";

import { store } from './store';
import { Provider } from 'react-redux';

import App from "./App";

class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { error: string | null }
> {
  constructor(props: any) {
    super(props);
    this.state = { error: null };
  }
  static getDerivedStateFromError(e: any) {
    return { error: String(e) };
  }
  componentDidCatch(e: any, info: any) {
    console.error('React error boundary caught:', e, info);
  }
  render() {
    if (this.state.error) {
      return (
        <div style={{
          padding: '2rem', color: '#f85149', background: '#0d1117',
          fontFamily: 'monospace', whiteSpace: 'pre-wrap', height: '100vh'
        }}>
          <h2>Render Error</h2>
          <p>{this.state.error}</p>
          <button onClick={() => this.setState({ error: null })}
            style={{ marginTop: '1rem', padding: '8px 16px', cursor: 'pointer' }}>
            Retry
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);

root.render(
  <React.StrictMode>
    <Provider store={store}>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </Provider>
  </React.StrictMode>
);