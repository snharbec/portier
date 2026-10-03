<script lang="ts">
	import type { SwipeAction } from '#lib/swipe.ts';
	import type { Snippet } from 'svelte';

	let {
		left,
		right,
		label,
		onaction,
		tinted = false,
		children
	}: {
		/** Actions for sliding left, and for sliding right. One is performed on release; several become buttons. */
		left: SwipeAction[];
		right: SwipeAction[];
		label: (action: SwipeAction) => string;
		onaction: (action: SwipeAction) => void;
		/** Row is selected: keep its tint on the sliding surface. */
		tinted?: boolean;
		children: Snippet;
	} = $props();

	const BUTTON = 112; // px per revealed button
	const START = 8; // px of horizontal travel before a press becomes a slide

	let element: HTMLDivElement;
	let offset = $state(0);
	let sliding = $state(false);
	let pointer: number | null = null;
	let startX = 0;
	let startY = 0;
	let base = 0;
	let width = 0;
	let suppressClick = false;

	const actions = $derived(offset > 0 ? right : offset < 0 ? left : []);
	/** Travel needed to perform a single action. */
	const trigger = () => Math.min(120, width * 0.3);
	const armed = $derived(actions.length === 1 && Math.abs(offset) >= trigger());

	function down(event: PointerEvent) {
		if (event.button !== 0 || !(left.length || right.length)) return;
		const target = event.target as HTMLElement;
		if (target.closest('.pane, .pick, .grip')) return;
		pointer = event.pointerId;
		startX = event.clientX;
		startY = event.clientY;
		base = offset;
		width = element.offsetWidth;
		suppressClick = false;
	}

	function move(event: PointerEvent) {
		if (event.pointerId !== pointer) return;
		const dx = event.clientX - startX;
		const dy = event.clientY - startY;
		if (!sliding) {
			if (Math.abs(dx) < START || Math.abs(dx) < Math.abs(dy)) return;
			sliding = true;
			suppressClick = true;
			element.setPointerCapture(event.pointerId);
		}
		let next = base + dx;
		if (next > 0 && !right.length) next = 0;
		if (next < 0 && !left.length) next = 0;
		const limit = width * 0.6;
		offset = Math.max(-limit, Math.min(limit, next));
	}

	function up(event: PointerEvent) {
		if (event.pointerId !== pointer) return;
		pointer = null;
		if (!sliding) return;
		sliding = false;
		const direction = Math.sign(offset);
		const offered = actions;
		if (offered.length === 1) {
			const perform = Math.abs(offset) >= trigger();
			offset = 0;
			if (perform) onaction(offered[0]);
		} else if (offered.length > 1 && Math.abs(offset) > 48) {
			// Stay open on the buttons until one is chosen or the row is slid back.
			offset = direction * BUTTON * offered.length;
		} else {
			offset = 0;
		}
	}

	function cancel() {
		pointer = null;
		sliding = false;
		offset = 0;
	}

	/** A slide must not also count as a click on the link underneath. */
	function click(event: MouseEvent) {
		if (suppressClick) {
			event.preventDefault();
			event.stopPropagation();
			suppressClick = false;
		} else if (offset !== 0 && !(event.target as HTMLElement).closest('.pane')) {
			// Row is open on its buttons: a click on the row closes it instead of opening the mail.
			event.preventDefault();
			event.stopPropagation();
			offset = 0;
		}
	}

	function choose(action: SwipeAction) {
		offset = 0;
		onaction(action);
	}
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="swipe"
	class:sliding
	bind:this={element}
	onpointerdown={down}
	onpointermove={move}
	onpointerup={up}
	onpointercancel={cancel}
	onclickcapture={click}
	ondragstart={(event) => {
		// Only the grip starts a real drag; elsewhere a mouse drag is the slide gesture.
		if (!(event.target as HTMLElement).closest('.grip')) event.preventDefault();
	}}
>
	{#each [{ side: 'start', list: right, shown: offset > 0 }, { side: 'end', list: left, shown: offset < 0 }] as pane}
		{#if pane.list.length && pane.shown}
			<div class="pane {pane.side}" class:armed>
				{#if pane.list.length === 1}
					<span class="hint {pane.list[0]}">{label(pane.list[0])}</span>
				{:else}
					{#each pane.list as action}
						<button class={action} style="width: {BUTTON}px" onclick={() => choose(action)}>{label(action)}</button>
					{/each}
				{/if}
			</div>
		{/if}
	{/each}
	<div class="content" class:tinted style="transform: translateX({offset}px)">
		{@render children()}
	</div>
</div>

<style>
	.swipe {
		position: relative;
		flex: 1;
		min-width: 0;
		overflow: hidden;
		/* Vertical scrolling stays with the browser; horizontal movement is ours. */
		touch-action: pan-y;
	}
	.content {
		position: relative;
		display: flex;
		background: var(--surface);
		transition: transform 0.18s ease-out;
	}
	.content.tinted {
		background: color-mix(in srgb, var(--important) 9%, var(--surface));
	}
	.sliding .content {
		transition: none;
		user-select: none;
	}
	.pane {
		position: absolute;
		inset: 0;
		display: flex;
		align-items: stretch;
	}
	.pane.end {
		justify-content: flex-end;
	}
	.hint,
	button {
		--c: var(--important);
		display: grid;
		place-items: center;
		padding: 0 1rem;
		border: 0;
		background: var(--c);
		color: #fff;
		font-weight: 700;
		font-size: 0.9rem;
		text-align: center;
	}
	.hint {
		flex: 1;
		opacity: 0.65;
	}
	.pane.start .hint {
		justify-content: start;
	}
	.pane.end .hint {
		justify-content: end;
	}
	.armed .hint {
		opacity: 1;
	}
	button {
		cursor: pointer;
	}
	.move {
		--c: var(--feed);
	}
	.archive {
		--c: var(--ink-soft);
	}
	.trash {
		--c: var(--junk);
	}
	@media (prefers-color-scheme: dark) {
		.hint,
		button {
			color: #0f1524;
		}
	}
</style>
