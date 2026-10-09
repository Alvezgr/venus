import { themes as prismThemes } from 'prism-react-renderer';
import type { Config } from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'Venus',
  tagline: 'Your Insomnia collections, in a native Rust app',
  favicon: 'img/favicon.ico',

  future: {
    v4: true,
  },

  // Themes and Plugins
  themes: [
    [
      require.resolve('@easyops-cn/docusaurus-search-local'),
      {
        hashed: true,
        language: ['en'],
        highlightSearchTermsOnTargetPage: true,
      },
    ],
  ],

  plugins: [
    'docusaurus-plugin-image-zoom',
  ],

  // GitHub Pages deployment
  url: 'https://alvezgr.github.io',
  baseUrl: '/venus/',

  organizationName: 'alvezgr',
  projectName: 'venus',
  trailingSlash: false,

  onBrokenLinks: 'throw',

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          editUrl: 'https://github.com/alvezgr/venus/tree/master/website/',
        },
        blog: false, // Disable blog
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/logo.png',

    // Image Zoom Configuration
    zoom: {
      selector: '.markdown :not(em) > img',
      config: {
        background: {
          light: 'rgb(255, 255, 255)',
          dark: 'rgb(10, 10, 10)'
        }
      }
    },

    colorMode: {
      defaultMode: 'dark',
      disableSwitch: false,
      respectPrefersColorScheme: false,
    },
    navbar: {
      title: 'Venus',
      logo: {
        alt: 'Venus Logo',
        src: 'img/logo.png',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docsSidebar',
          position: 'left',
          label: 'Docs',
        },
        {
          to: '/docs/quickstart',
          label: 'Quick Start',
          position: 'left',
        },
        {
          to: '/docs/getting-started',
          label: 'Build & install',
          position: 'right',
        },
        {
          href: 'https://github.com/alvezgr/venus',
          label: 'GitHub',
          position: 'right',
        },
        {
          type: 'search',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Learn',
          items: [
            {
              label: 'Getting Started',
              to: '/docs/getting-started',
            },
            {
              label: 'Quick Start',
              to: '/docs/quickstart',
            },
            {
              label: 'Features',
              to: '/docs/features/requests',
            },
          ],
        },
        {
          title: 'Reference',
          items: [
            {
              label: 'File Format (JSON)',
              to: '/docs/reference/file-format',
            },
            {
              label: 'Keyboard Shortcuts',
              to: '/docs/reference/keyboard-shortcuts',
            },
          ],
        },
        {
          title: 'More',
          items: [
            {
              label: 'GitHub',
              href: 'https://github.com/alvezgr/venus',
            },
            {
              label: 'Releases',
              href: 'https://github.com/alvezgr/venus/releases',
            },
            {
              label: 'Issues',
              href: 'https://github.com/alvezgr/venus/issues',
            },
          ],
        },
      ],
      copyright: `Venus is an independent fork of Mercury by Harshit Chaudhary (@Harry-kp). MIT licensed.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ['bash', 'json', 'toml'],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
