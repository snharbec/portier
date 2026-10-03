<script lang="ts">
	import { page } from '$app/state';
	import ThreadView from '#lib/components/ThreadView.svelte';
</script>

{#key page.params.id}
	<ThreadView id={page.params.id ?? ''} />
{/key}
