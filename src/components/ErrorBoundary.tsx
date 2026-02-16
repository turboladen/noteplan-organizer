import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

/**
 * Top-level error boundary that prevents the entire app from going blank
 * when a component throws during rendering. In a Tauri desktop app, users
 * can't easily refresh the page like in a browser, so this recovery UI
 * is critical for a usable experience.
 */
export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Uncaught error in React tree:", error, info.componentStack);
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="min-h-screen bg-surface flex items-center justify-center">
          <div className="text-center max-w-md px-6">
            <h1 className="text-xl font-semibold text-text-primary mb-2">
              Something went wrong
            </h1>
            <p className="text-text-tertiary mb-4">
              The app encountered an unexpected error.
            </p>
            {this.state.error && (
              <pre className="text-xs text-red-600 bg-red-50 border border-red-200 rounded-[var(--radius-card)] px-3 py-2 mb-4 whitespace-pre-wrap text-left">
                {this.state.error.message}
              </pre>
            )}
            <button
              onClick={() => window.location.reload()}
              className="px-4 py-2 bg-accent text-white text-sm font-medium rounded-[var(--radius-button)] hover:bg-accent-hover transition-colors shadow-sm"
            >
              Reload App
            </button>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
