<script lang="ts">
	import type { Selection } from '#lib/selection.svelte.ts';

	let {
		selection,
		selected,
		total,
		onall,
		onnone
	}: {
		selection: Selection;
		/** How many of the shown mails are ticked, and how many are shown. */
		selected: number;
		total: number;
		onall: () => void;
		onnone: () => void;
	} = $props();

	const all = $derived(total > 0 && selected === total);
	const some = $derived(selected > 0 && selected < total);

	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'Escape' && selection.active && !document.querySelector('dialog[open]')) selection.stop();
	}
</script>

<svelte:window {onkeydown} />

<div class="select-all">
	{#if selection.active}
		<label>
			<span class="pick">
				<input
					type="checkbox"
					checked={all}
					indeterminate={some}
					onchange={(event) => (event.currentTarget.checked ? onall() : onnone())}
				/>
			</span>
			{all ? 'Deselect all' : 'Select all'}
			<span class="muted">{selected ? `${selected} of ${total} selected` : `${total} shown`}</span>
		</label>
		<button class="btn small quiet" onclick={selection.stop}>Stop selecting</button>
	{:else}
		<button class="btn small" onclick={selection.start}>Select</button>
		<span class="muted">{total} shown</span>
	{/if}
</div>

<style>
	.select-all,
	label {
		display: inline-flex;
		align-items: center;
		gap: 0.1rem 0.5rem;
	}
	.select-all {
		height: 2.25rem;
		/* Lines the checkbox up with the row checkboxes inside the list's border. */
		margin: 0 0 0.35rem 1px;
		font-size: 0.9rem;
	}
	label {
		font-weight: 600;
		cursor: pointer;
	}
	.muted {
		font-weight: 400;
	}
</style>
