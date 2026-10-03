<script lang="ts">
	import { hue, initials } from '#lib/format.ts';

	let {
		name,
		seed,
		size = 44,
		address
	}: {
		name: string;
		seed: string;
		size?: number;
		/** Sender address to look up a picture for (Gravatar, or the domain's BIMI logo). */
		address?: string | null;
	} = $props();

	// Initials show at once; the picture replaces them when it turns out to exist.
	let loaded = $state(false);
	let failed = $state(false);
	const src = $derived(address && address.includes('@') ? `/api/avatar?address=${encodeURIComponent(address)}` : null);

	$effect(() => {
		src;
		loaded = false;
		failed = false;
	});
</script>

<span class="avatar" class:picture={loaded} style="--h: {hue(seed)}; --size: {size}px" aria-hidden="true">
	{#if !loaded}{initials(name)}{/if}
	{#if src && !failed}
		<img {src} alt="" loading="lazy" class:shown={loaded} onload={() => (loaded = true)} onerror={() => (failed = true)} />
	{/if}
</span>

<style>
	.avatar {
		position: relative;
		flex: none;
		display: grid;
		place-items: center;
		width: var(--size);
		height: var(--size);
		border-radius: 50%;
		overflow: hidden;
		background: hsl(var(--h) 62% 86%);
		color: hsl(var(--h) 55% 24%);
		font: 700 calc(var(--size) * 0.38) var(--display);
	}
	/* Logos are often transparent and drawn for a light background. */
	.avatar.picture {
		background: #fff;
		box-shadow: inset 0 0 0 1px var(--line);
	}
	img {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		object-fit: cover;
		opacity: 0;
	}
	img.shown {
		opacity: 1;
	}
</style>
