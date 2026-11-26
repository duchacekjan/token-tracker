<script lang="ts">
  import type { SessionFilter, SessionSummary } from "../types";

  interface Props {
    sessions: SessionSummary[];
    initial: SessionFilter;
    onclose: () => void;
  }
  let { sessions, initial, onclose }: Props = $props();
</script>

<div class="state">
  Export placeholder ({sessions.length} sessions, source {initial.source})
  <button onclick={onclose}>Close</button>
</div>
