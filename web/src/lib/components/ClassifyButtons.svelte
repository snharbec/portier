<script lang="ts">
	import type { Category } from '#lib/api.ts';
	import { categoryNames } from '#lib/app.svelte.ts';

	let {
		current = null,
		onpick,
		small = false
	}: { current?: Category | null; onpick: (category: Category) => void; small?: boolean } = $props();

	const order: Category[] = ['important', 'feed', 'junk'];
</script>

<div class="choices" class:small>
	{#each order as category}
		<button class={category} aria-pressed={current === category} onclick={() => onpick(category)}>
			{categoryNames[category]}
		</button>
	{/each}
</div>

<style>
	.choices {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}
	button {
		--c: var(--important);
		padding: 0.5rem 1rem;
		border: 2px solid var(--c);
		border-radius: 999px;
		background: transparent;
		color: var(--c);
		font-weight: 700;
		cursor: pointer;
	}
	.small button {
		padding: 0.2rem 0.7rem;
		font-size: 0.85rem;
		border-width: 1.5px;
	}
	button.feed {
		--c: var(--feed);
	}
	button.junk {
		--c: var(--junk);
	}
	button:hover,
	button[aria-pressed='true'] {
		background: var(--c);
		color: var(--surface);
	}
</style>
