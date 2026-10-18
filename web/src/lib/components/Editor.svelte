<script lang="ts">
	import { Editor } from '@tiptap/core';
	import Placeholder from '@tiptap/extension-placeholder';
	import StarterKit from '@tiptap/starter-kit';
	import { onMount } from 'svelte';

	let {
		value = $bindable(''),
		onchange,
		autofocus = false,
		placeholder = 'Write your message',
		label = 'Message'
	}: {
		value?: string;
		onchange?: () => void;
		/** Puts the cursor into the text as soon as the editor is there. */
		autofocus?: boolean;
		placeholder?: string;
		/** What the text field is called for a screen reader. */
		label?: string;
	} = $props();

	let element: HTMLDivElement;
	let editor: Editor | undefined = $state();
	let revision = $state(0);
	/** The cursor was put into the text at some point: what is inserted goes where it was left. */
	let touched = false;

	onMount(() => {
		editor = new Editor({
			element,
			extensions: [StarterKit, Placeholder.configure({ placeholder })],
			content: value,
			editorProps: { attributes: { 'aria-label': label, 'aria-multiline': 'true', role: 'textbox' } },
			autofocus: autofocus ? 'end' : false,
			onUpdate: ({ editor }) => {
				value = editor.isEmpty ? '' : editor.getHTML();
				onchange?.();
			},
			onTransaction: () => (revision += 1),
			onFocus: () => (touched = true)
		});
		return () => editor?.destroy();
	});

	/** Puts formatted text where the cursor is (at the end when the text was never clicked into). */
	export function insert(html: string) {
		if (!editor) return;
		editor
			.chain()
			.focus(touched ? undefined : 'end')
			.insertContent(html)
			.run();
	}

	const tools = [
		{ label: 'Bold', mark: 'bold', run: (e: Editor) => e.chain().focus().toggleBold().run() },
		{ label: 'Italic', mark: 'italic', run: (e: Editor) => e.chain().focus().toggleItalic().run() },
		{ label: 'List', mark: 'bulletList', run: (e: Editor) => e.chain().focus().toggleBulletList().run() },
		{ label: 'Numbered', mark: 'orderedList', run: (e: Editor) => e.chain().focus().toggleOrderedList().run() },
		{ label: 'Quote', mark: 'blockquote', run: (e: Editor) => e.chain().focus().toggleBlockquote().run() }
	];

	/** Whether the text at the cursor has this format; `revision` makes it follow each change. */
	function active(mark: string, revision: number) {
		return revision >= 0 && (editor?.isActive(mark) ?? false);
	}

	function addLink() {
		if (!editor) return;
		if (editor.isActive('link')) {
			editor.chain().focus().unsetLink().run();
			return;
		}
		const url = window.prompt('Link address');
		if (url) editor.chain().focus().setLink({ href: url }).run();
	}
</script>

<!-- The buttons take no focus from the text (mousedown), so a click formats where the cursor is;
     they stay the same elements while the text changes, else a click on one would be lost. -->
<div class="toolbar" role="toolbar" aria-label="Formatting">
	{#each tools as tool}
		<button
			type="button"
			aria-pressed={active(tool.mark, revision)}
			onmousedown={(event) => event.preventDefault()}
			onclick={() => editor && tool.run(editor)}
		>
			{tool.label}
		</button>
	{/each}
	<button type="button" aria-pressed={active('link', revision)} onmousedown={(event) => event.preventDefault()} onclick={addLink}>
		Link
	</button>
</div>
<div class="editor" bind:this={element}></div>

<style>
	.toolbar {
		display: flex;
		flex-wrap: wrap;
		gap: 0.25rem;
		padding: 0.5rem 0;
		border-bottom: 1px solid var(--line);
	}
	button {
		padding: 0.2rem 0.6rem;
		border: 0;
		border-radius: 6px;
		background: none;
		font-size: 0.875rem;
		font-weight: 600;
		color: var(--ink-soft);
		cursor: pointer;
	}
	button:hover,
	button[aria-pressed='true'] {
		background: color-mix(in srgb, var(--important) 14%, transparent);
		color: var(--ink);
	}
	.editor :global(.tiptap) {
		min-height: 14rem;
		padding: 1rem 0;
		outline: none;
		font-size: 1.05rem;
	}
	.editor :global(.tiptap p) {
		margin: 0 0 0.75rem;
	}
	.editor :global(.tiptap blockquote) {
		margin: 0 0 0.75rem;
		padding-left: 0.9rem;
		border-left: 3px solid var(--line);
		color: var(--ink-soft);
	}
	.editor :global(.tiptap p.is-editor-empty:first-child::before) {
		content: attr(data-placeholder);
		color: var(--ink-soft);
		float: left;
		height: 0;
		pointer-events: none;
	}
	/* Big enough for a finger. */
	@media (pointer: coarse) {
		.toolbar button {
			min-height: 2.5rem;
			min-width: 2.75rem;
		}
	}
</style>
