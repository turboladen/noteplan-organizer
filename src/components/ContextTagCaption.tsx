interface ContextTagCaptionProps {
  tags: string[];
}

/**
 * Caption shown under the context tabs (Board + Backlog) naming the tags that
 * scope calendar tasks into the active context. Renders nothing when the
 * context declares no tags.
 */
export function ContextTagCaption({ tags }: ContextTagCaptionProps) {
  if (tags.length === 0) return null;
  return (
    <p className="text-xs text-text-tertiary -mt-2 mb-4">
      Calendar tasks tagged{" "}
      {tags.map((t) => (
        <span key={t} className="text-text-secondary">
          #{t}{" "}
        </span>
      ))}
      appear under this context.
    </p>
  );
}
