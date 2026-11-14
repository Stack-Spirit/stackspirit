import { useProjectFilter, useFleet, useFleetStore } from "../../state/fleetStore";

export function ProjectSwitcher() {
  const fleet = useFleet();
  const projectFilter = useProjectFilter();
  const setProject = useFleetStore((state) => state.setProject);

  return (
    <label className="project-switcher">
      <span className="eyebrow">Project</span>
      <select
        className="select"
        value={projectFilter ?? ""}
        aria-label="Project filter"
        disabled={!fleet}
        onChange={(event) => setProject(event.target.value === "" ? null : event.target.value)}
      >
        <option value="">All projects</option>
        {(fleet?.projects ?? []).map((project) => (
          <option key={project.name} value={project.name}>
            {project.name}
            {project.state !== "active" ? ` (${project.state})` : ""}
          </option>
        ))}
      </select>
    </label>
  );
}
