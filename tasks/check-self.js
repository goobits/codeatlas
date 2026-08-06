#!/usr/bin/env node

const fs = require('node:fs')
const path = require('node:path')
const crypto = require('node:crypto')
const { spawnSync } = require('node:child_process')
const { requireExternalCargoTarget, writePrivateFile } = require('./storage.js')

const rootDir = path.resolve(__dirname, '..')
const maxOutputBytes = 32 * 1024 * 1024
const inspectedCallable = {
	query: 'src/commands/output.rs#write_text_or_print',
	nodeId:
		'symbol/file~1codeatlas~1src~01commands~01output.rs/rs:src~1commands~1output.rs:fn#write_text_or_print',
	digest: '3624d781f1dbf4d5ec324c65966d118aa1e95b82cb24127ed3190fca6517dc8c',
	effectSource:
		'symbol/file~1codeatlas~1src~01filesystem.rs/rs:src~1filesystem.rs:fn#replace_file'
}
const semanticSiblingSetIds = [
	'http_source_detectors',
	'language_adapters',
	'repository_term_adapters'
]
const semanticSiblingCounterevidenceKinds = [
	'conflicting_or_unknown_effects',
	'different_authority_or_security_boundaries',
	'different_lifecycle_or_cleanup_ownership',
	'incompatible_result_or_error_semantics',
	'disjoint_producer_or_consumer_roles',
	'different_externally_owned_protocol_obligations',
	'distinct_configured_concepts',
	'incomplete_graph_or_type_evidence'
]

const countBy = (items, field) => {
	const counts = new Map()
	for (const item of items ?? []) {
		const value = String(item?.[field] ?? 'unknown')
		counts.set(value, (counts.get(value) ?? 0) + 1)
	}
	return Object.fromEntries([...counts].sort(([left], [right]) => left.localeCompare(right)))
}

const collectSymbols = (roots) => {
	const pending = [...(roots ?? [])].reverse()
	const symbols = []
	while (pending.length > 0) {
		const symbol = pending.pop()
		symbols.push(symbol)
		for (const child of [...(symbol.children ?? [])].reverse()) pending.push(child)
	}
	return symbols
}

const digestJson = (value) =>
	crypto.createHash('sha256').update(JSON.stringify(value)).digest('hex')

const resolveInspectedCallable = (report) => {
	const target = report.targets?.find((candidate) => candidate.query === inspectedCallable.query)
	if (!target || target.nodes?.length !== 1) return undefined
	const nodeId = target.nodes[0]
	return { nodeId, callable: report.nodes?.[nodeId]?.callable }
}

const summarize = (id, report) => {
	switch (id) {
		case 'scan-code': {
			const symbols = collectSymbols(report.symbols)
			const callables = symbols.map((symbol) => symbol.callable).filter(Boolean)
			const signatures = callables.flatMap((callable) => callable.signatures ?? [])
			const parameters = signatures.flatMap((signature) => signature.parameters ?? [])
			const effects = callables.flatMap((callable) => callable.effects ?? [])
			const blockReasons = callables.flatMap((callable) => callable.block_reasons ?? [])
			return {
				files: report.stats?.files_scanned ?? 0,
				skipped: report.stats?.files_skipped ?? 0,
				symbols: report.stats?.symbols_found ?? 0,
				callables: callables.length,
				effects: countBy(effects, 'kind'),
				blockReasons: countBy(blockReasons, 'kind'),
				parameterConstructibility: countBy(parameters, 'constructibility'),
				receiverConstructibility: countBy(
					signatures.map((signature) => signature.receiver),
					'constructibility'
				)
			}
		}
		case 'check-code':
		case 'usage-code':
			return {
				findings: report.findings?.length ?? 0,
				gates: report.findings?.filter((finding) => finding.gates).length ?? 0,
				kinds: countBy(report.findings, 'kind')
			}
		case 'inspect-code': {
			const inspected = resolveInspectedCallable(report)
			return {
				targets: report.targets?.length ?? 0,
				nodes: Object.keys(report.nodes ?? {}).length,
				edges: report.edges?.length ?? 0,
				omitted: report.omitted ?? {},
				targetNode: inspected?.nodeId ?? null,
				callableDigest: inspected?.callable ? digestJson(inspected.callable) : null,
				effects: inspected?.callable?.effects ?? [],
				blockReasons: inspected?.callable?.block_reasons ?? []
			}
		}
		case 'lexicon-code':
			return report.stats ?? {}
		case 'lexicon-repository':
			return {
				terms: report.terms?.length ?? 0,
				relationships: report.relationships?.length ?? 0,
				subjects: Object.fromEntries(
					(report.subjects ?? []).map((subject) => [
						subject.subject,
						{
							evidence: subject.evidenceCount ?? 0,
							complete: subject.completeness?.complete ?? false
						}
					])
				),
				omittedRelationshipEvidence:
					report.relationships?.reduce(
						(total, relationship) => total + (relationship.omittedEvidence ?? 0),
						0
					) ?? 0
			}
		case 'tests-inventory':
			return {
				projects: report.projects?.length ?? 0,
				contexts:
					report.projects?.reduce(
						(total, project) => total + (project.contexts?.length ?? 0),
						0
					) ?? 0,
				scripts:
					report.projects?.reduce(
						(total, project) => total + (project.scripts?.length ?? 0),
						0
					) ?? 0,
				duplicateScripts: report.duplicate_scripts?.length ?? 0
			}
		case 'tests-witnesses':
			return report.summary ?? {}
		default:
			return {}
	}
}

const validateInspectedCallable = (report) => {
	const inspected = resolveInspectedCallable(report)
	if (!inspected) return ['did not resolve exactly one inspected callable']
	const failures = []
	if (inspected.nodeId !== inspectedCallable.nodeId) {
		failures.push(`resolved unexpected node ${inspected.nodeId}`)
	}
	if (!inspected.callable) return [...failures, 'omitted the structured callable contract']
	const digest = digestJson(inspected.callable)
	if (digest !== inspectedCallable.digest) {
		failures.push(`callable digest changed from ${inspectedCallable.digest} to ${digest}`)
	}
	const expectedEffect = inspected.callable.effects?.some(
		(effect) =>
			effect.kind === 'filesystem_write' &&
			effect.provenance?.kind === 'propagated' &&
			effect.provenance?.source_target === inspectedCallable.effectSource
	)
	if (!expectedEffect) failures.push('omitted the propagated filesystem-write effect')
	const expectedBlock = inspected.callable.block_reasons?.some(
		(reason) => reason.kind === 'requires_factory' && reason.subject === 'signature:0:parameter:1'
	)
	if (!expectedBlock) failures.push('omitted the exact Path factory block')
	return failures
}

const validateSemanticSiblings = (report) => {
	const analysis = report.semantic_sibling_analysis
	if (!analysis || !Array.isArray(analysis.comparison_sets)) {
		return ['omitted semantic_sibling_analysis.comparison_sets']
	}
	const failures = []
	const ids = analysis.comparison_sets.map((set) => set.id)
	if (JSON.stringify(ids) !== JSON.stringify(semanticSiblingSetIds)) {
		failures.push(`resolved unexpected semantic sibling sets ${JSON.stringify(ids)}`)
	}

	let evaluationCount = 0
	let reviewCandidateCount = 0
	let omittedCount = 0
	for (const set of analysis.comparison_sets) {
		const evaluations = set.evaluations ?? []
		evaluationCount += evaluations.length
		if (set.nominations_considered !== evaluations.length) {
			failures.push(`${set.id} nomination and evaluation counts disagree`)
		}
		const omissionSum = (set.omissions ?? []).reduce(
			(total, omission) => total + (omission.count ?? 0),
			0
		)
		omittedCount += omissionSum
		if (set.omitted_nominations !== omissionSum) {
			failures.push(`${set.id} omission aggregate disagrees with its evidence`)
		}
		for (const evaluation of evaluations) {
			const kinds = (evaluation.counterevidence_checks ?? []).map((check) => check.kind)
			if (JSON.stringify(kinds) !== JSON.stringify(semanticSiblingCounterevidenceKinds)) {
				failures.push(`${set.id} evaluation omitted the mandatory counterevidence checklist`)
			}
			if (evaluation.corroboration_count !== (evaluation.corroborations ?? []).length) {
				failures.push(`${set.id} evaluation corroboration aggregate disagrees with its evidence`)
			}
			if (evaluation.disposition === 'review_candidate') reviewCandidateCount += 1
			for (const key of Object.keys(evaluation)) {
				if (key === 'gate' || key === 'gates') {
					failures.push(`${set.id} evaluation exposed forbidden gating state`)
				}
			}
		}
	}

	const stats = report.stats ?? {}
	for (const [field, actual] of [
		['semantic_sibling_comparison_sets', analysis.comparison_sets.length],
		['semantic_sibling_evaluations', evaluationCount],
		['semantic_sibling_review_candidates', reviewCandidateCount],
		['semantic_sibling_omitted_nominations', omittedCount]
	]) {
		if (stats[field] !== actual) failures.push(`${field} disagrees with report evidence`)
	}
	return [...new Set(failures)]
}

const validateRepositoryLexicon = (report) => {
	const failures = []
	const subjects = (report.subjects ?? []).map((subject) => subject.subject)
	if (JSON.stringify(subjects) !== JSON.stringify(['code', 'http', 'postgres'])) {
		failures.push(`resolved unexpected repository lexicon subjects ${JSON.stringify(subjects)}`)
	}
	for (const relationship of report.relationships ?? []) {
		const retained = relationship.evidence?.length ?? 0
		if (relationship.claim !== 'related_evidence') {
			failures.push(`relationship ${relationship.id ?? '<missing>'} asserted ${relationship.claim}`)
		}
		if (retained > 128) {
			failures.push(`relationship ${relationship.id ?? '<missing>'} exceeded its evidence bound`)
		}
		if (relationship.evidenceCount !== retained + (relationship.omittedEvidence ?? 0)) {
			failures.push(
				`relationship ${relationship.id ?? '<missing>'} retained/omitted counts disagree`
			)
		}
	}
	const serialized = JSON.stringify(report)
	if (serialized.includes('semanticEquivalence') || serialized.includes('semantic_equivalence')) {
		failures.push('repository relationships exposed forbidden semantic-equivalence state')
	}
	return failures
}

const run = (command, args, options = {}) => {
	const result = spawnSync(command, args, {
		cwd: rootDir,
		encoding: 'utf8',
		env: { ...process.env, NO_COLOR: '1' },
		maxBuffer: maxOutputBytes
	})
	if (result.error) throw result.error
	if (result.signal) throw new Error(`${options.label ?? command} terminated by ${result.signal}`)
	return result
}

try {
	const targetDir = requireExternalCargoTarget(rootDir)
	const auditDir = path.join(targetDir, 'codeatlas-self-audit')
	fs.mkdirSync(auditDir, { recursive: true, mode: 0o700 })
	fs.chmodSync(auditDir, 0o700)

	const build = run('cargo', ['build', '--quiet', '--locked', '--bin', 'codeatlas'], {
		label: 'CodeAtlas self-audit build'
	})
	writePrivateFile(path.join(auditDir, 'build.stdout.log'), build.stdout)
	writePrivateFile(path.join(auditDir, 'build.stderr.log'), build.stderr)
	if (build.status !== 0) {
		throw new Error(`CodeAtlas self-audit build failed with exit ${build.status ?? 1}`)
	}

	const executable = path.join(
		targetDir,
		'debug',
		process.platform === 'win32' ? 'codeatlas.exe' : 'codeatlas'
	)
	const globalArgs = ['--root', '.', '--config', 'codeatlas.json']
	const commands = [
		{
			id: 'scan-code',
			args: ['scan', 'code', '--scope', 'source', '--all', '--format', 'json']
		},
		{ id: 'check-code', args: ['check', 'code', '--format', 'json'], check: true },
		{ id: 'usage-code', args: ['usage', 'code', '--format', 'json'] },
		{
			id: 'inspect-code',
			args: [
				'inspect',
				'code',
				inspectedCallable.query,
				'--depth',
				'1',
				'--direction',
				'outgoing',
				'--max-nodes',
				'32'
			],
			validate: validateInspectedCallable
		},
		{
			id: 'lexicon-code',
			args: ['lexicon', 'code', '--format', 'json'],
			validate: validateSemanticSiblings
		},
		{
			id: 'lexicon-repository',
			args: ['lexicon', 'repository', '--format', 'json'],
			schemaVersion: 'codeatlas.repository-lexicon/v1',
			validate: validateRepositoryLexicon
		},
		{ id: 'tests-inventory', args: ['scan', 'tests', '--format', 'json'] },
		{
			id: 'tests-witnesses',
			args: ['check', 'tests', '--format', 'json'],
			check: true
		}
	]
	const failures = []
	const summaries = []

	for (const descriptor of commands) {
		const result = run(executable, [...globalArgs, ...descriptor.args], {
			label: descriptor.id
		})
		const stdoutBytes = Buffer.byteLength(result.stdout)
		const stderrBytes = Buffer.byteLength(result.stderr)
		writePrivateFile(path.join(auditDir, `${descriptor.id}.json`), result.stdout)
		writePrivateFile(path.join(auditDir, `${descriptor.id}.stderr.log`), result.stderr)

		let report
		try {
			report = JSON.parse(result.stdout)
		} catch (error) {
			failures.push(`${descriptor.id} did not emit valid JSON: ${error.message}`)
			continue
		}
		const schemaVersion = descriptor.schemaVersion ? report.schemaVersion : report.schema_version
		if (descriptor.schemaVersion) {
			if (report.schemaVersion !== descriptor.schemaVersion) {
				failures.push(`${descriptor.id} omitted schema ${descriptor.schemaVersion}`)
			}
		} else if (!Number.isInteger(report.schema_version)) {
			failures.push(`${descriptor.id} omitted an integer schema_version`)
		}
		for (const failure of descriptor.validate?.(report) ?? []) {
			failures.push(`${descriptor.id} ${failure}`)
		}
		if (result.status !== 0) {
			const reason = descriptor.check
				? 'reported gating findings'
				: `failed with exit ${result.status ?? 1}`
			failures.push(`${descriptor.id} ${reason}`)
		}

		const summary = summarize(descriptor.id, report)
		summaries.push({
			id: descriptor.id,
			exitCode: result.status ?? 1,
			reportBytes: stdoutBytes,
			diagnosticBytes: stderrBytes,
			schemaVersion,
			summary
		})
		console.log(`${descriptor.id}: ${JSON.stringify(summary)}`)
	}

	writePrivateFile(
		path.join(auditDir, 'summary.json'),
		`${JSON.stringify({ schemaVersion: 1, reports: summaries }, null, 2)}\n`
	)
	console.log(`CodeAtlas self-audit artifacts: ${auditDir}`)
	if (failures.length > 0) throw new Error(failures.join('\n'))
} catch (error) {
	console.error(error instanceof Error ? error.message : String(error))
	process.exitCode = 1
}
