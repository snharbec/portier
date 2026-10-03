<script lang="ts">
	let {
		selected,
		total,
		onall,
		onnone
	}: {
		/** How many of the shown mails are ticked, and how many are shown. */
		selected: number;
		total: number;
		onall: () => void;
		onnone: () => void;
	} = $props();

	const all = $derived(total > 0 && selected === total);
	const some = $derived(selected > 0 && selected < total);
</script>

<label class="select-all">
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

<style>
	.select-all {
		display: inline-flex;
		align-items: center;
		gap: 0.1rem 0.5rem;
		height: 2.25rem;
		/* Lines the checkbox up with the row checkboxes inside the list's border. */
		margin: 0 0 0.35rem 1px;
		font-weight: 600;
		font-size: 0.9rem;
		cursor: pointer;
	}
	.muted {
		font-weight: 400;
	}
</style>
