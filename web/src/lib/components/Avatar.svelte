<script lang="ts">
	import { hue, initials } from '#lib/format.ts';

	let { name, seed, size = 44 }: { name: string; seed: string; size?: number } = $props();
</script>

<span class="avatar" style="--h: {hue(seed)}; --size: {size}px" aria-hidden="true">{initials(name)}</span>

<style>
	.avatar {
		flex: none;
		display: grid;
		place-items: center;
		width: var(--size);
		height: var(--size);
		border-radius: 50%;
		background: hsl(var(--h) 62% 86%);
		color: hsl(var(--h) 55% 24%);
		font: 700 calc(var(--size) * 0.38) var(--display);
	}
</style>
