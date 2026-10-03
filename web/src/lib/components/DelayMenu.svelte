<script lang="ts">
	let { onpick, disabled = false, up = false }: { onpick: (days: number) => void; disabled?: boolean; up?: boolean } =
		$props();

	const choices = [
		{ days: 1, label: '1 day' },
		{ days: 2, label: '2 days' },
		{ days: 3, label: '3 days' },
		{ days: 7, label: '7 days' }
	];
	let open = $state(false);
	let root: HTMLSpanElement;

	function pick(days: number) {
		open = false;
		onpick(days);
	}

	/** Opens the choices; used by the keyboard shortcut. */
	export function show() {
		open = true;
		queueMicrotask(() => root.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus());
	}

	function onwindowclick(event: MouseEvent) {
		if (open && !root.contains(event.target as Node)) open = false;
	}

	function onkeydown(event: KeyboardEvent) {
		if (!open) return;
		if (event.key === 'Escape') {
			open = false;
			event.stopPropagation();
		}
		// With the choices open, the digit of a choice picks it.
		const choice = choices.find((c) => String(c.days) === event.key);
		if (choice) {
			event.preventDefault();
			event.stopPropagation();
			pick(choice.days);
		}
	}
</script>

<svelte:window onclick={onwindowclick} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<span class="delay" bind:this={root} {onkeydown}>
	<button class="btn small" {disabled} aria-haspopup="menu" aria-expanded={open} onclick={() => (open = !open)}>
		Delay
	</button>
	{#if open}
		<span class="choices" class:up role="menu" aria-label="Return to the inbox, unread, at 7:00 in">
			<span class="hint">Back in the inbox at 7:00 in</span>
			{#each choices as choice}
				<button role="menuitem" onclick={() => pick(choice.days)}>{choice.label}</button>
			{/each}
		</span>
	{/if}
</span>

<style>
	.delay {
		position: relative;
		display: inline-block;
	}
	.choices {
		position: absolute;
		top: calc(100% + 0.3rem);
		left: 0;
		z-index: 40;
		display: grid;
		min-width: 13rem;
		padding: 0.35rem;
		border: 1px solid var(--line);
		border-radius: 12px;
		background: var(--surface);
		color: var(--ink);
		box-shadow: 0 18px 40px -18px color-mix(in srgb, var(--ink) 60%, transparent);
	}
	.choices.up {
		top: auto;
		bottom: calc(100% + 0.3rem);
	}
	.hint {
		padding: 0.3rem 0.6rem 0.4rem;
		font-size: 0.8rem;
		color: var(--ink-soft);
	}
	.choices button {
		padding: 0.4rem 0.6rem;
		border: 0;
		border-radius: 8px;
		background: none;
		text-align: left;
		font-weight: 600;
		cursor: pointer;
	}
	.choices button:hover,
	.choices button:focus-visible {
		background: var(--paper);
	}
</style>
