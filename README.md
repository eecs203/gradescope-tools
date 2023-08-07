# `gradescope-tools`

A collection of apps and libraries to help large courses automate tasks involving Gradescope.

Created at [EECS 203](https://eecs203.github.io/eecs203.org/) at the University of Michigan.

## Libraries

- [`gradescope-api`](gradescope-api/): Scraper and parser for Gradescope data, such as assignments and regrade requests
- [`lib203`](lib203/): Abstraction over `gradescope-api` with specific EECS 203 concepts, such as Individual Homework and Groupwork
- [`app-utils`](app-utils/): Common app utilities

## Apps

- [`gradescope-to-db`](gradescope-to-db/): Scrape course data into a database for easier access and analysis
- [`notify-unmatched-pages`](notify-unmatched-pages/): Notify submitters who have not matched pages for an assignment
