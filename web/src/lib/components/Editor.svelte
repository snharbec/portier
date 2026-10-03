<script lang="ts">
	import { Editor } from '@tiptap/core';
	import Placeholder from '@tiptap/extension-placeholder';
	import StarterKit from '@tiptap/starter-kit';
	import { onMount } from 'svelte';

	let { value = $bindable(''), onchange }: { value?: string; onchange?: () => void } = $props();

	let element: HTMLDivElement;
	let editor: Editor | undefined = $state();
	let revision = $state(0);

	onMount(() => {
		editor = new Editor({
			element,
			extensions: [StarterKit, Placeholder.configure({ placeholder: 'Write your message' })],
			content: value,
			onUpdate: ({ editor }) => {
				value = editor.isEmpty ? '' : editor.getHTML();
				onchange?.();
			},
			onTransaction: () => (revision += 1)
		});
		return () => editor?.destroy();
	});

	const tools = [
		{ label: 'Bold', mark: 'bold', run: (e: Editor) => e.chain().focus().toggleBold().run() },
		{ label: 'Italic', mark: 'italic', run: (e: Editor) => e.chain().focus().toggleItalic().run() },
		{ label: 'List', mark: 'bulletList', run: (e: Editor) => e.chain().focus().toggleBulletList().run() },
		{ label: 'Numbered', mark: 'orderedList', run: (e: Editor) => e.chain().focus().toggleOrderedList().run() },
		{ label: 'Quote', mark: 'blockquote', run: (e: Editor) => e.chain().focus().toggleBlockquote().run() }
	];

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

<div class="toolbar" role="toolbar" aria-label="Formatting">
	{#each tools as tool}
		{#key revision}
			<button type="button" aria-pressed={editor?.isActive(tool.mark) ?? false} onclick={() => editor && tool.run(editor)}>
				{tool.label}
			</button>
		{/key}
	{/each}
	{#key revision}
		<button type="button" aria-pressed={editor?.isActive('link') ?? false} onclick={addLink}>Link</button>
	{/key}
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
</style>
