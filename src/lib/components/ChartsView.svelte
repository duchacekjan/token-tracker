<script lang="ts">
  import type { SessionSummary } from "../types";

  interface Props {
    sessions: SessionSummary[];
  }
  let { sessions }: Props = $props();
</script>

<div class="state">Charts placeholder ({sessions.length} sessions)</div>
