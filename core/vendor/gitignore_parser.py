import os
from pathlib import Path
from fnmatch import fnmatch

class Matcher:
    """
    A simple and robust gitignore matcher.
    """
    def __init__(self, lines, base_dir):
        self.base_dir = Path(base_dir).resolve()
        self.rules = self._compile_rules(lines)

    def _compile_rules(self, lines):
        """Compiles gitignore patterns into a structured list of rules."""
        compiled = []
        for line in lines:
            line = line.strip()
            if not line or line.startswith('#'):
                continue
            
            is_negation = line.startswith('!')
            if is_negation:
                line = line[1:]

            # A pattern with a separator in the middle is anchored to the base_dir.
            # A pattern starting with a slash is also anchored.
            is_anchored = "/" in line.strip('/')

            # A trailing slash means the pattern should only match directories.
            is_dir_only = line.endswith('/')
            
            # Normalize the pattern by removing trailing/leading slashes for matching.
            pattern_to_match = line.strip('/')

            compiled.append({
                "pattern": pattern_to_match,
                "is_negation": is_negation,
                "is_dir_only": is_dir_only,
                "is_anchored": is_anchored,
            })
        return compiled

    def __call__(self, path_to_check):
        """Checks if a given path should be ignored based on the compiled rules."""
        path_to_check = Path(path_to_check).resolve()
        
        if path_to_check == self.base_dir:
            return False

        try:
            relative_path = path_to_check.relative_to(self.base_dir)
        except ValueError:
            return False

        is_dir = path_to_check.is_dir()
        
        # Gitignore matching uses POSIX-style paths.
        relative_path_posix = relative_path.as_posix()
        
        ignored = False
        for rule in self.rules:
            # If a rule is for directories only, skip if the path is not a directory.
            if rule["is_dir_only"] and not is_dir:
                continue

            pattern = rule["pattern"]
            
            match = False
            # Anchored patterns match against the full relative path.
            # Non-anchored patterns match against any component of the path.
            if rule["is_anchored"]:
                if fnmatch(relative_path_posix, pattern):
                    match = True
            else:
                if any(fnmatch(part, pattern) for part in relative_path.parts):
                    match = True
            
            if match:
                if rule["is_negation"]:
                    ignored = False
                else:
                    ignored = True
        
        return ignored