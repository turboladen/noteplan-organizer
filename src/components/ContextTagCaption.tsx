interface ContextTagCaptionProps {
  tags: string[];
}

/**
 * Caption shown under the context tabs (Board + Backlog) naming the tags that
 * scope calendar tasks into the active context. Renders nothing when the
 * context declares no tags.
 */
export function ContextTagCaption({ tags }: ContextTagCaptionProps) {
  // Dedupe: a context can declare the same tag twice, which would otherwise
  // collide on the React key and render a redundant chip.
  const unique = [...new Set(tags)];
  if (unique.length === 0) return null;
  return (
    <p className="text-xs text-text-tertiary -mt-2 mb-4">
      Calendar tasks with any of these tags appear under this context:{" "}
      {unique.map((t) => (
        <span key={t} className="text-text-secondary">
          #{t}{" "}
        </span>
      ))}
    </p>
  );
}
