<script lang="ts">
	let dialog: HTMLDialogElement;

	/** Opens the list of keyboard shortcuts, or closes it when it is open. */
	export function toggle() {
		if (dialog.open) dialog.close();
		else dialog.showModal();
	}

	const groups = [
		{
			title: 'Everywhere',
			keys: [
				[['?'], 'Show this list'],
				[['c'], 'Write a mail'],
				[['/'], 'Search'],
				[['m'], 'Open the menu'],
				[['1'], 'Inbox'],
				[['Shift', 'I'], 'Important'],
				[['Shift', 'D'], 'Delayed'],
				[['Shift', 'N'], 'Nice to know (also 3)'],
				[['2'], 'Screener'],
				[['4'], 'Attachments']
			]
		},
		{
			title: 'In a mail list',
			keys: [
				[['↓'], 'Next mail: opens it in split view, otherwise moves to its row'],
				[['↑'], 'Previous mail'],
				[['Enter'], 'Open the mail of the row'],
				[['Esc'], 'Stop selecting']
			]
		},
		{
			title: 'Reading a mail',
			keys: [
				[['↓'], 'Next mail'],
				[['↑'], 'Previous mail'],
				[['Space'], 'Page down (also Ctrl ↓)'],
				[['Backspace'], 'Page up (also Shift Space, Ctrl ↑)'],
				[['r'], 'Reply'],
				[['a'], 'Reply to all'],
				[['f'], 'Forward'],
				[['e'], 'Archive'],
				[['d'], 'Move to Trash'],
				[['u'], 'Mark as unseen'],
				[['i'], 'Move to Important, or back to the Inbox'],
				[['z'], 'Delay, then 1, 2, 3 or 7 for the days'],
				[['n'], 'Next search result'],
				[['p'], 'Previous search result']
			]
		},
		{
			title: 'Writing a mail',
			keys: [[['Ctrl', 'Return'], 'Send the mail']]
		},
		{
			title: 'Screener',
			keys: [
				[['i'], 'First sender goes to the Inbox'],
				[['k'], 'First sender is nice to know'],
				[['j'], 'First sender is junk']
			]
		},
		{
			title: 'Attachment viewer',
			keys: [
				[['←'], 'Previous file'],
				[['→'], 'Next file'],
				[['Esc'], 'Close']
			]
		}
	] as { title: string; keys: [string[], string][] }[];

	function onkeydown(event: KeyboardEvent) {
		if (event.key === '?') {
			event.preventDefault();
			// The page's own handler would open it again.
			event.stopPropagation();
			dialog.close();
		}
	}
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<dialog bind:this={dialog} aria-labelledby="keys-title" {onkeydown} onclick={(event) => event.target === dialog && dialog.close()}>
	<div class="head">
		<h2 id="keys-title">Keyboard shortcuts</h2>
		<button class="btn small" onclick={() => dialog.close()}>Close</button>
	</div>
	<div class="groups">
		{#each groups as group}
			<section>
				<h3>{group.title}</h3>
				<dl>
					{#each group.keys as [keys, what]}
						<div>
							<dt>
								{#each keys as key}<kbd>{key}</kbd>{/each}
							</dt>
							<dd>{what}</dd>
						</div>
					{/each}
				</dl>
			</section>
		{/each}
	</div>
</dialog>

<style>
	dialog {
		width: min(46rem, 100vw - 2rem);
		max-height: min(44rem, 100dvh - 2rem);
		padding: 1.4rem;
		border: 1px solid var(--line);
		border-radius: var(--radius);
		background: var(--surface);
		color: var(--ink);
	}
	dialog::backdrop {
		background: color-mix(in srgb, var(--ink) 45%, transparent);
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 1rem;
		margin-bottom: 0.4rem;
	}
	/* Two columns of groups where there is room; a group is never split between them. */
	.groups {
		columns: 19rem 2;
		column-gap: 2rem;
	}
	section {
		break-inside: avoid;
		padding-top: 0.9rem;
	}
	h3 {
		font: 600 0.85rem var(--body);
		letter-spacing: 0;
		color: var(--ink-soft);
		margin-bottom: 0.3rem;
	}
	dl {
		margin: 0;
	}
	dl div {
		display: grid;
		grid-template-columns: 6.5rem 1fr;
		gap: 0.6rem;
		align-items: baseline;
		padding: 0.22rem 0;
	}
	dt {
		display: flex;
		gap: 0.25rem;
	}
	dd {
		margin: 0;
		font-size: 0.92rem;
	}
	kbd {
		font: 600 0.8rem var(--body);
		min-width: 1.5rem;
		text-align: center;
		border: 1px solid var(--line);
		border-bottom-width: 2px;
		border-radius: 6px;
		padding: 0.05rem 0.4rem;
		background: var(--paper);
		white-space: nowrap;
	}
</style>
