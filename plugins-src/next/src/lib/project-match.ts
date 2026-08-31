export interface ProjectExample {
  projects: readonly string[]
  text: string
}

export interface ProjectSuggestion {
  project: string
  reason: 'name' | 'content'
  score: number
  matchedTerms: readonly string[]
  /** Number of projects touched by the query's inverted-index postings. */
  candidatesScored: number
}

export interface ProjectMatcher {
  recommend(text: string): ProjectSuggestion | null
}

interface Posting {
  project: string
  weight: number
}

const LATIN_MIN = 3

function normalize(value: string): string {
  return value.normalize('NFKC').toLocaleLowerCase().replace(/\s+/g, ' ').trim()
}

function termsOf(value: string): string[] {
  const terms = new Set<string>()
  for (const segment of normalize(value).match(/[\p{Script=Han}]+|[\p{Letter}\p{Number}]+/gu) ?? []) {
    if (/^\p{Script=Han}+$/u.test(segment)) {
      if (segment.length === 1) continue
      for (let size = 2; size <= Math.min(3, segment.length); size += 1) {
        for (let index = 0; index <= segment.length - size; index += 1) terms.add(segment.slice(index, index + size))
      }
    } else if (segment.length >= LATIN_MIN) {
      terms.add(segment)
    }
  }
  return [...terms]
}

function containsProjectName(text: string, project: string): boolean {
  const name = normalize(project)
  if (!name) return false
  if (/\p{Script=Han}/u.test(name)) return name.length >= 2 && text.includes(name)
  if (name.replace(/[^\p{Letter}\p{Number}]/gu, '').length < LATIN_MIN) return false
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  return new RegExp(`(^|[^\\p{Letter}\\p{Number}])${escaped}($|[^\\p{Letter}\\p{Number}])`, 'u').test(text)
}

/**
 * Builds one local inverted index. Training evidence only comes from ideas
 * whose project tags were previously confirmed by a person.
 */
export function buildProjectMatcher(
  examples: readonly ProjectExample[],
  projectOptions: readonly string[],
): ProjectMatcher {
  const projects: string[] = []
  const projectByKey = new Map<string, string>()
  for (const value of projectOptions) {
    const project = value.normalize('NFKC').trim()
    const key = normalize(project)
    if (!project || projectByKey.has(key)) continue
    projects.push(project)
    projectByKey.set(key, project)
  }
  const documentCount = new Map<string, number>(projects.map((project) => [project, 0]))
  const termCounts = new Map<string, Map<string, number>>(projects.map((project) => [project, new Map()]))

  for (const example of examples) {
    const terms = new Set(termsOf(example.text))
    if (!terms.size) continue
    const confirmed = new Set(example.projects.map((value) => projectByKey.get(normalize(value))).filter((value): value is string => Boolean(value)))
    for (const project of confirmed) {
      documentCount.set(project, (documentCount.get(project) ?? 0) + 1)
      const counts = termCounts.get(project)!
      for (const term of terms) counts.set(term, (counts.get(term) ?? 0) + 1)
    }
  }

  const projectFrequency = new Map<string, number>()
  for (const counts of termCounts.values()) {
    for (const term of counts.keys()) projectFrequency.set(term, (projectFrequency.get(term) ?? 0) + 1)
  }

  const postings = new Map<string, Posting[]>()
  for (const project of projects) {
    const documents = documentCount.get(project) ?? 0
    if (!documents) continue
    for (const [term, count] of termCounts.get(project) ?? []) {
      const idf = 1 + Math.log((projects.length + 1) / ((projectFrequency.get(term) ?? 0) + 1))
      const weight = idf * (0.5 + 0.5 * count / documents)
      const list = postings.get(term) ?? []
      list.push({ project, weight })
      postings.set(term, list)
    }
  }

  return {
    recommend(value: string): ProjectSuggestion | null {
      const text = normalize(value)
      if (!text || !projects.length) return null

      const nameMatches = projects
        .filter((project) => containsProjectName(text, project))
        .sort((left, right) => normalize(right).length - normalize(left).length)
      if (nameMatches.length && (!nameMatches[1] || normalize(nameMatches[0]).length > normalize(nameMatches[1]).length)) {
        return { project: nameMatches[0], reason: 'name', score: 100, matchedTerms: [nameMatches[0]], candidatesScored: nameMatches.length }
      }

      const scores = new Map<string, { score: number; terms: Map<string, number> }>()
      for (const term of termsOf(text)) {
        for (const posting of postings.get(term) ?? []) {
          const current = scores.get(posting.project) ?? { score: 0, terms: new Map() }
          current.score += posting.weight
          current.terms.set(term, posting.weight)
          scores.set(posting.project, current)
        }
      }
      const ranked = [...scores]
        .map(([project, result]) => ({ project, ...result }))
        .sort((left, right) => right.score - left.score || left.project.localeCompare(right.project))
      const best = ranked[0]
      const second = ranked[1]
      if (!best || best.terms.size < 2 || best.score < 2.2) return null
      if (second && (best.score - second.score < 0.35 || best.score < second.score * 1.2)) return null

      return {
        project: best.project,
        reason: 'content',
        score: best.score,
        matchedTerms: [...best.terms]
          .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
          .slice(0, 5)
          .map(([term]) => term),
        candidatesScored: ranked.length,
      }
    },
  }
}
