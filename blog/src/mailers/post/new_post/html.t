{% extends "base.t" %}
{% block title %}{{ title }}{% endblock %}
{% block body %}
<h1>{{ title }}</h1>
{% if reading_minutes %}<p><em>{{ reading_minutes }} min read</em></p>{% endif %}
<p>{{ excerpt }}</p>
<p><a href="{{ url }}">Read the post &rarr;</a></p>
{% endblock %}
{% block footer %}
You get this because you subscribed to Loco Blog.
<a href="{{ unsubscribe_url }}">Unsubscribe</a>.
{% endblock %}
