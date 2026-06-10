// Signal Feed — the only default view (§4.1). Virtualized from day one (§3.6).
import { Virtuoso } from "react-virtuoso";
import { useUi2Store } from "../store/store";
import { FeedRowView } from "./FeedRow";

export function SignalFeed() {
  const rows = useUi2Store((s) => s.feed.rows);

  return (
    <section className="ui2-feed" role="feed" aria-label="Signal feed">
      {rows.length === 0 ? (
        <p className="ui2-meta ui2-feed-empty">No signal yet.</p>
      ) : (
        <Virtuoso
          data={rows}
          computeItemKey={(_, row) => row.key}
          itemContent={(_, row) => <FeedRowView row={row} />}
          followOutput="smooth"
          initialItemCount={Math.min(rows.length, 30)}
          aria-label="Signal feed messages"
        />
      )}
    </section>
  );
}
