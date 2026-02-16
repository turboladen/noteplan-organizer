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
        <div className="min-h-screen bg-gray-50 flex items-center justify-center">
          <div className="text-center max-w-md px-6">
            <div className="text-4xl mb-4">Something went wrong</div>
            <p className="text-gray-600 mb-2">
              The app encountered an unexpected error.
            </p>
            {this.state.error && (
              <pre className="text-xs text-red-600 bg-red-50 border border-red-200 rounded px-3 py-2 mb-4 whitespace-pre-wrap text-left">
                {this.state.error.message}
              </pre>
            )}
            <button
              onClick={() => window.location.reload()}
              className="px-4 py-2 bg-gray-900 text-white text-sm rounded-lg hover:bg-gray-800 transition-colors"
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
